use crossterm::{ExecutableCommand, event};
use std::fmt::Write;

use mirl_buffer::Buffer;
use mirl_extensions::*;
use mirl_input::keyboard::KeyCode;
use mirl_input::mouse::MouseButton;
use mirl_system::traits::WindowRenderLayer;

use mirl_windowing_core::windowing::{WindowSettings, errors::*, traits::*};

/// Backend implementation using just the console
#[derive(Debug)]
#[cfg_attr(feature = "c_compatible", repr(C))]
pub struct Framework {
    /// The title of the current window
    pub name: String,
    /// If the usual os menu should be shown
    pub os_menu: bool,
    /// If the title is visible
    pub visible_title: bool,
    /// If the contents should even be displayed at all
    pub visible: bool,
    /// If the window is still open
    pub is_open: bool,
    /// The width of the buffer it should hold
    pub width: usize,
    /// The height of the buffer it should hold
    pub height: usize,
    /// The the size of the buffer last frame, needed to smartly clear the screen
    pub last_size: (usize, usize),
    /// The recorded mouse position
    pub mouse_pos: std::sync::Arc<std::sync::Mutex<(f32, f32)>>,
    /// If any of the typical mouse buttons are detected
    pub mouse_buttons: std::sync::Arc<std::sync::Mutex<[bool; 3]>>,
    /// All currently pressed keys
    pub pressed_keys: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<KeyCode>>>,
}

impl NewWindow for Framework {
    fn new(
        title: &str,
        settings: WindowSettings,
        // #[cfg(feature = "svg")] cursor: Option<Cursor>,
    ) -> Result<Self, WindowError>
    where
        Self: Sized,
    {
        crossterm::execute!(std::io::stdout(), event::EnableMouseCapture).map_err(|x| {
            WindowError::Misc(format!(
                "Error while trying to activate mouse capturing: {x}"
            ))
        })?;
        Ok(Self {
            name: title.to_string(),
            os_menu: settings.os_menu,
            visible: settings.visible,
            visible_title: settings.title_visible,
            height: settings.size.0 as usize,
            width: settings.size.1 as usize,
            last_size: (0, 0),
            is_open: true,
            mouse_pos: std::sync::Arc::new(std::sync::Mutex::new((0.0, 0.0))),
            mouse_buttons: std::sync::Arc::new(std::sync::Mutex::new([false; 3])),

            pressed_keys: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )),
        })
    }
}

impl Framework {
    /// Poll for mouse and keyboard events
    fn poll_events(&self) -> Result<(), WindowError> {
        if event::poll(std::time::Duration::from_millis(100))
            .map_err(|x| WindowError::Misc(format!("Error while trying to poll events: {x}")))?
        {
            match event::read() {
                Ok(crossterm::event::Event::Mouse(mouse_event)) => {
                    // Update position
                    *self.mouse_pos.lock().map_err(|x| {
                        WindowError::Misc(format!("Error while trying to lock mouse pos: {x}"))
                    })? = (f32::from(mouse_event.column), f32::from(mouse_event.row));

                    // Update button states
                    match mouse_event.kind {
                        crossterm::event::MouseEventKind::Down(btn) => {
                            let idx = button_to_index(btn);
                            self.mouse_buttons.lock().map_err(|x| {
                                WindowError::Misc(format!(
                                    "Error while trying to lock mouse buttons (1): {x}"
                                ))
                            })?[idx.to_number::<usize>()] = true;
                        }
                        crossterm::event::MouseEventKind::Up(btn) => {
                            let idx = button_to_index(btn);
                            self.mouse_buttons.lock().map_err(|x| {
                                WindowError::Misc(format!(
                                    "Error while trying to lock mouse buttons (2): {x}"
                                ))
                            })?[idx.to_number::<usize>()] = false;
                        }
                        _ => {}
                    }
                }

                Ok(crossterm::event::Event::Key(key_event)) => {
                    let key = crossterm_to_keycode(key_event.code);
                    let mut keys = self.pressed_keys.lock().map_err(|x| {
                        WindowError::Misc(format!("Error while trying to lock pressed keys: {x}"))
                    })?;
                    match key_event.kind {
                        crossterm::event::KeyEventKind::Press => {
                            keys.insert(key);
                        }
                        crossterm::event::KeyEventKind::Release => {
                            keys.remove(&key);
                        }
                        crossterm::event::KeyEventKind::Repeat => {}
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    return Err(WindowError::Misc(format!(
                        "Error while trying to read event: {e}"
                    )));
                }
            }
        }
        Ok(())
    }
}

impl MouseInput for Framework {
    fn get_mouse_position(&self) -> Option<(f32, f32)> {
        Some(*self.mouse_pos.lock().ok()?)
    }

    fn is_mouse_down(&self, button: MouseButton) -> bool {
        let idx = match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
            _ => return false,
        };
        self.mouse_buttons.lock().is_ok_and(|x| x[idx])
    }
}
#[must_use]
/// Convert a crossterm mouse button event into a mirl mouse button
pub const fn button_to_index(btn: crossterm::event::MouseButton) -> mirl_input::mouse::MouseButton {
    match btn {
        crossterm::event::MouseButton::Left => mirl_input::mouse::MouseButton::Left,
        crossterm::event::MouseButton::Right => mirl_input::mouse::MouseButton::Right,
        crossterm::event::MouseButton::Middle => mirl_input::mouse::MouseButton::Middle,
    }
}
impl Window for Framework {
    fn update_raw(
        &mut self,
        pixels: &[u32],
        width: usize,
        height: usize,
    ) -> Result<(), WindowError> {
        let new = mirl_buffer_interpolation::resize_buffer_nearest(
            pixels,
            width,
            height,
            self.width,
            self.height,
        );
        let (w, h) = crossterm::terminal::size()
            .map_err(|_x| WindowError::Misc("Unable to get console size".to_string()))?;
        let w = w as usize - 2;
        let h = h as usize - 2;
        if w < self.width {
            let ratio = self.width as f32 / self.height as f32;
            self.width = w;
            self.height = (w as f32 / ratio).round() as usize;
        }
        if h < self.height {
            let ratio = self.width as f32 / self.height as f32;
            self.height = h;
            self.width = (h as f32 / ratio).round() as usize;
        }

        let data = mirl_terminal::color_data_to_console(&new, self.width, self.height);

        if self.last_size != (self.width, self.height) {
            std::io::stdout()
                .execute(crossterm::terminal::Clear(
                    crossterm::terminal::ClearType::All,
                ))
                .map_err(|x| WindowUpdateError::Misc(format!("Unable to clear screen: {x}")))?;
            self.last_size = (self.width, self.height);
        }
        std::io::stdout()
            .execute(crossterm::cursor::MoveTo(0, 0))
            .map_err(|x| WindowUpdateError::Misc(format!("Unable to move cursor: {x}")))?;

        let mut final_text = format!("> {}", self.name);
        let _ = write!(
            final_text,
            "\n+{}+",
            '-'.repeat_value(self.width).concatenate()
        );
        for line in data {
            let _ = write!(final_text, "\n|{line}|");
        }
        let _ = write!(
            final_text,
            "\n+{}+",
            '-'.repeat_value(self.width).concatenate()
        );

        println!("{final_text}");
        self.poll_events()?;

        Ok(())
    }

    fn is_open(&self) -> bool {
        self.is_open
    }
    fn close_and_clean_up(&mut self) {
        // TODO: ADD PROPER ERRORING
        let _ = crossterm::execute!(std::io::stdout(), event::DisableMouseCapture).map_err(|x| {
            WindowError::Misc(format!(
                "Error while trying to activate mouse capturing: {x}"
            ))
        });
        self.is_open = false;
    }
}

impl RenderLayer for Framework {
    #[inline]
    fn set_render_layer(&mut self, _level: WindowRenderLayer) {
        // We can only have 1 window at the time
    }
}

impl Control for Framework {
    fn get_position(&self) -> (i32, i32) {
        (0, 0)
    }

    fn set_position(&mut self, _xy: (i32, i32)) {}

    fn set_size(&mut self, buffer: &Buffer) {
        self.width = buffer.width;
        self.height = buffer.height;
    }

    fn get_size(&self) -> (i32, i32) {
        (self.width as i32, self.height as i32)
    }
}
// impl Timing for Framework {
//     #[inline]
//     fn get_time(&self) -> Box<dyn Time> {
//         shared::get_time()
//     }
//     #[inline]
//     fn sleep(&self, time: std::time::Duration) {
//         shared::sleep(time);
//     }
// }

impl Output for Framework {
    #[inline]
    fn log(&self, t: &str) {
        t.println_self();
    }
}

impl Visibility for Framework {
    #[inline]
    fn maximize(&mut self) {
        let Ok((w, h)) = crossterm::terminal::size()
            .map_err(|_x| WindowError::Misc("Unable to get console size".to_string()))
        else {
            return;
        };
        let w = w as usize;
        let h = h as usize;

        if w > self.width {
            let ratio = self.height as f32 / self.width as f32;
            self.width = w;
            self.height = (w as f32 / ratio).round() as usize;
        }
        if h < self.height {
            let ratio = self.width as f32 / self.height as f32;
            self.height = h;
            self.width = (h as f32 / ratio).round() as usize;
        }
    }
    #[inline]
    fn minimize(&mut self) {
        self.visible = false;
    }
    #[inline]
    fn restore(&mut self) {
        self.visible = true;
    }
    fn is_maximized(&self) -> bool {
        let Ok((w, _h)) = crossterm::terminal::size()
            .map_err(|_x| WindowError::Misc("Unable to get console size".to_string()))
        else {
            return false;
        };
        let w = w as usize;
        w == self.width + 2
    }
    fn is_minimized(&self) -> bool {
        !self.visible
    }
}

impl ExtendedWindow for Framework {
    fn set_title(&mut self, title: &str) {
        self.name = title.to_string();
    }
}
#[must_use]
/// Convert a crossterm keycode event into a mirl keycode
pub const fn crossterm_to_keycode(key: crossterm::event::KeyCode) -> KeyCode {
    match key {
        crossterm::event::KeyCode::Char('a' | 'A') => KeyCode::KeyA,
        crossterm::event::KeyCode::Char('b' | 'B') => KeyCode::KeyB,
        crossterm::event::KeyCode::Char('c' | 'C') => KeyCode::KeyC,
        crossterm::event::KeyCode::Char('d' | 'D') => KeyCode::KeyD,
        crossterm::event::KeyCode::Char('e' | 'E') => KeyCode::KeyE,
        crossterm::event::KeyCode::Char('f' | 'F') => KeyCode::KeyF,
        crossterm::event::KeyCode::Char('g' | 'G') => KeyCode::KeyG,
        crossterm::event::KeyCode::Char('h' | 'H') => KeyCode::KeyH,
        crossterm::event::KeyCode::Char('i' | 'I') => KeyCode::KeyI,
        crossterm::event::KeyCode::Char('j' | 'J') => KeyCode::KeyJ,
        crossterm::event::KeyCode::Char('k' | 'K') => KeyCode::KeyK,
        crossterm::event::KeyCode::Char('l' | 'L') => KeyCode::KeyL,
        crossterm::event::KeyCode::Char('m' | 'M') => KeyCode::KeyM,
        crossterm::event::KeyCode::Char('n' | 'N') => KeyCode::KeyN,
        crossterm::event::KeyCode::Char('o' | 'O') => KeyCode::KeyO,
        crossterm::event::KeyCode::Char('p' | 'P') => KeyCode::KeyP,
        crossterm::event::KeyCode::Char('q' | 'Q') => KeyCode::KeyQ,
        crossterm::event::KeyCode::Char('r' | 'R') => KeyCode::KeyR,
        crossterm::event::KeyCode::Char('s' | 'S') => KeyCode::KeyS,
        crossterm::event::KeyCode::Char('t' | 'T') => KeyCode::KeyT,
        crossterm::event::KeyCode::Char('u' | 'U') => KeyCode::KeyU,
        crossterm::event::KeyCode::Char('v' | 'V') => KeyCode::KeyV,
        crossterm::event::KeyCode::Char('w' | 'W') => KeyCode::KeyW,
        crossterm::event::KeyCode::Char('x' | 'X') => KeyCode::KeyX,
        crossterm::event::KeyCode::Char('y' | 'Y') => KeyCode::KeyY,
        crossterm::event::KeyCode::Char('z' | 'Z') => KeyCode::KeyZ,
        crossterm::event::KeyCode::Char('0') => KeyCode::Num0,
        crossterm::event::KeyCode::Char('1') => KeyCode::Num1,
        crossterm::event::KeyCode::Char('2') => KeyCode::Num2,
        crossterm::event::KeyCode::Char('3') => KeyCode::Num3,
        crossterm::event::KeyCode::Char('4') => KeyCode::Num4,
        crossterm::event::KeyCode::Char('5') => KeyCode::Num5,
        crossterm::event::KeyCode::Char('6') => KeyCode::Num6,
        crossterm::event::KeyCode::Char('7') => KeyCode::Num7,
        crossterm::event::KeyCode::Char('8') => KeyCode::Num8,
        crossterm::event::KeyCode::Char('9') => KeyCode::Num9,
        crossterm::event::KeyCode::Enter => KeyCode::Enter,
        crossterm::event::KeyCode::Esc => KeyCode::Escape,
        crossterm::event::KeyCode::Backspace => KeyCode::Backspace,
        crossterm::event::KeyCode::Tab => KeyCode::Tab,
        crossterm::event::KeyCode::Char(' ') => KeyCode::Space,
        crossterm::event::KeyCode::Left => KeyCode::LeftArrow,
        crossterm::event::KeyCode::Right => KeyCode::RightArrow,
        crossterm::event::KeyCode::Up => KeyCode::UpArrow,
        crossterm::event::KeyCode::Down => KeyCode::DownArrow,
        crossterm::event::KeyCode::F(1) => KeyCode::F1,
        crossterm::event::KeyCode::F(2) => KeyCode::F2,
        crossterm::event::KeyCode::F(3) => KeyCode::F3,
        crossterm::event::KeyCode::F(4) => KeyCode::F4,
        crossterm::event::KeyCode::F(5) => KeyCode::F5,
        crossterm::event::KeyCode::F(6) => KeyCode::F6,
        crossterm::event::KeyCode::F(7) => KeyCode::F7,
        crossterm::event::KeyCode::F(8) => KeyCode::F8,
        crossterm::event::KeyCode::F(9) => KeyCode::F9,
        crossterm::event::KeyCode::F(10) => KeyCode::F10,
        crossterm::event::KeyCode::F(11) => KeyCode::F11,
        crossterm::event::KeyCode::F(12) => KeyCode::F12,
        crossterm::event::KeyCode::Insert => KeyCode::Insert,
        crossterm::event::KeyCode::Delete => KeyCode::Delete,
        crossterm::event::KeyCode::Home | event::KeyCode::KeypadBegin => KeyCode::Home,
        crossterm::event::KeyCode::End => KeyCode::End,
        crossterm::event::KeyCode::PageUp => KeyCode::PageUp,
        crossterm::event::KeyCode::PageDown => KeyCode::PageDown,
        crossterm::event::KeyCode::Char('-') => KeyCode::Minus,
        crossterm::event::KeyCode::Char('=') => KeyCode::Equal,
        crossterm::event::KeyCode::Char('[') => KeyCode::LeftBracket,
        crossterm::event::KeyCode::Char(']') => KeyCode::RightBracket,
        crossterm::event::KeyCode::Char('\\') => KeyCode::Backslash,
        crossterm::event::KeyCode::Char(';') => KeyCode::Semicolon,
        crossterm::event::KeyCode::Char('\'') => KeyCode::Quote,
        crossterm::event::KeyCode::Char(',') => KeyCode::Comma,
        crossterm::event::KeyCode::Char('.') => KeyCode::Period,
        crossterm::event::KeyCode::Char('/') => KeyCode::Slash,
        crossterm::event::KeyCode::Char('`') => KeyCode::Grave,
        event::KeyCode::BackTab => KeyCode::BackTab,
        event::KeyCode::F(_) | event::KeyCode::Char(_) | event::KeyCode::Null => KeyCode::Unknown,
        event::KeyCode::CapsLock => KeyCode::CapsLock,
        event::KeyCode::ScrollLock => KeyCode::ScrollLock,
        event::KeyCode::NumLock => KeyCode::NumLock,
        event::KeyCode::PrintScreen => KeyCode::PrintScreen,
        event::KeyCode::Pause => KeyCode::Pause,
        event::KeyCode::Menu => KeyCode::Menu,
        event::KeyCode::Media(media_key_code) => match media_key_code {
            event::MediaKeyCode::Play => KeyCode::MediaPlay,
            event::MediaKeyCode::Pause => KeyCode::MediaPause,
            event::MediaKeyCode::PlayPause => KeyCode::MediaPlayPause,
            event::MediaKeyCode::Reverse | event::MediaKeyCode::Rewind => KeyCode::MediaReverse, // I'm breaking the [KeyCode] rules but until someone explains to me the difference between rewind and reverse, I am keeping it like this
            event::MediaKeyCode::Stop => KeyCode::MediaStop,
            event::MediaKeyCode::FastForward => KeyCode::MediaFastForward,

            event::MediaKeyCode::TrackNext => KeyCode::MediaNext,
            event::MediaKeyCode::TrackPrevious => KeyCode::MediaPrev,
            event::MediaKeyCode::Record => KeyCode::MediaRecord,
            event::MediaKeyCode::LowerVolume => KeyCode::VolumeDown,
            event::MediaKeyCode::RaiseVolume => KeyCode::VolumeUp,
            event::MediaKeyCode::MuteVolume => KeyCode::Mute,
        },
        event::KeyCode::Modifier(modifier_key_code) => match modifier_key_code {
            event::ModifierKeyCode::LeftShift => KeyCode::LeftShift,
            event::ModifierKeyCode::LeftControl | event::ModifierKeyCode::LeftMeta => {
                KeyCode::LeftControl
            }
            event::ModifierKeyCode::LeftAlt => KeyCode::LeftAlt,
            event::ModifierKeyCode::LeftSuper => KeyCode::LeftSuper,
            event::ModifierKeyCode::LeftHyper => KeyCode::LeftHyper,
            event::ModifierKeyCode::RightShift => KeyCode::RightShift,
            event::ModifierKeyCode::RightControl | event::ModifierKeyCode::RightMeta => {
                KeyCode::RightControl
            }
            event::ModifierKeyCode::RightAlt => KeyCode::RightAlt,
            event::ModifierKeyCode::RightSuper => KeyCode::RightSuper,
            event::ModifierKeyCode::RightHyper => KeyCode::RightHyper,
            event::ModifierKeyCode::IsoLevel3Shift => KeyCode::AltControl,
            event::ModifierKeyCode::IsoLevel5Shift => KeyCode::SpecialControl,
        },
    }
}

impl KeyboardInput for Framework {
    fn is_key_down(&self, key: KeyCode) -> bool {
        self.pressed_keys.lock().unwrap().contains(&key)
    }
}
