// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src-tauri/src/flow/os_windows.rs, translated to macOS from NotchBuddy/Sources/App/{FlowShortcut,FlowKeyLogic,FlowDictation}.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Flow dictation on macOS (Next Agent's translation; Coucou's Tauri app has
// Windows and Linux only). The keys: the held state of each key, read about
// a hundred times a second from the session's keyboard state, which needs no
// event tap and so no Accessibility prompt just to listen (the Mac app reads
// modifier flags in FlowShortcut.swift). The paste: ⌘V posted to the app in
// front, as FlowDictation.swift does; macOS delivers it once Accessibility is
// allowed. The app in front and the running apps: NSWorkspace.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use next_agent_macos_panel::sys;

use super::logic::{Combo, ComboEdge};
use super::KeyEvent;

const KEY_ESCAPE: u16 = 0x35;
/// How often the keys are read.
const TICK: Duration = Duration::from_millis(10);

/// A KeyboardEvent.code name → its macOS virtual key code (ANSI layout,
/// Carbon's Events.h).
pub fn code_key(code: &str) -> Option<u16> {
    let key = match code {
        "ControlLeft" => 0x3B,
        "ControlRight" => 0x3E,
        "AltLeft" => 0x3A,
        "AltRight" => 0x3D,
        "ShiftLeft" => 0x38,
        "ShiftRight" => 0x3C,
        "MetaLeft" => 0x37,
        "MetaRight" => 0x36,
        "CapsLock" => 0x39,
        "Space" => 0x31,
        "Backquote" => 0x32,
        "ContextMenu" => 0x6E,
        "F1" => 0x7A,
        "F2" => 0x78,
        "F3" => 0x63,
        "F4" => 0x76,
        "F5" => 0x60,
        "F6" => 0x61,
        "F7" => 0x62,
        "F8" => 0x64,
        "F9" => 0x65,
        "F10" => 0x6D,
        "F11" => 0x67,
        "F12" => 0x6F,
        "F13" => 0x69,
        "F14" => 0x6B,
        "F15" => 0x71,
        "F16" => 0x6A,
        "F17" => 0x40,
        "F18" => 0x4F,
        "F19" => 0x50,
        "F20" => 0x5A,
        other => {
            if let Some(letter) = other.strip_prefix("Key") {
                return letter_key(letter);
            }
            if let Some(digit) = other.strip_prefix("Digit") {
                return digit_key(digit);
            }
            return None;
        }
    };
    Some(key)
}

fn letter_key(letter: &str) -> Option<u16> {
    const LETTERS: [(&str, u16); 26] = [
        ("A", 0x00),
        ("S", 0x01),
        ("D", 0x02),
        ("F", 0x03),
        ("H", 0x04),
        ("G", 0x05),
        ("Z", 0x06),
        ("X", 0x07),
        ("C", 0x08),
        ("V", 0x09),
        ("B", 0x0B),
        ("Q", 0x0C),
        ("W", 0x0D),
        ("E", 0x0E),
        ("R", 0x0F),
        ("Y", 0x10),
        ("T", 0x11),
        ("O", 0x1F),
        ("U", 0x20),
        ("I", 0x22),
        ("P", 0x23),
        ("L", 0x25),
        ("J", 0x26),
        ("K", 0x28),
        ("N", 0x2D),
        ("M", 0x2E),
    ];
    LETTERS.iter().find(|(l, _)| *l == letter).map(|(_, k)| *k)
}

fn digit_key(digit: &str) -> Option<u16> {
    const DIGITS: [(&str, u16); 10] = [
        ("1", 0x12),
        ("2", 0x13),
        ("3", 0x14),
        ("4", 0x15),
        ("6", 0x16),
        ("5", 0x17),
        ("9", 0x19),
        ("7", 0x1A),
        ("8", 0x1C),
        ("0", 0x1D),
    ];
    DIGITS.iter().find(|(d, _)| *d == digit).map(|(_, k)| *k)
}

/// The modifier keys (Shift, Control, Option, Command, Caps Lock, Fn): never
/// "another key".
fn is_modifier(key: u16) -> bool {
    (0x36..=0x3F).contains(&key)
}

/// The listening thread; it ends when dropped.
pub struct KeyListener {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for KeyListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Keys can be watched in every macOS session.
pub fn keys_supported() -> Result<(), &'static str> {
    Ok(())
}

/// Watches the keys `codes` (KeyboardEvent.code names), held together to dictate.
pub fn listen(codes: &[String], tx: Sender<(KeyEvent, Instant)>) -> Result<KeyListener, String> {
    let keys: Vec<u16> = codes.iter().filter_map(|c| code_key(c)).collect();
    if keys.is_empty() || keys.len() != codes.len() {
        return Err(format!("unknown keys: {codes:?}"));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("coucou-flow-keys".into())
        .spawn(move || {
            let mut combo = Combo::new(keys.len());
            let mut held = vec![false; keys.len()];
            // Every other key, so a press is seen once, on its way down.
            let mut others = [false; 128];
            while !stop2.load(Ordering::Relaxed) {
                std::thread::sleep(TICK);
                for (i, &key) in keys.iter().enumerate() {
                    let down = sys::key_down(key);
                    if down == held[i] {
                        continue;
                    }
                    held[i] = down;
                    let edge = combo.key(i, down);
                    let event = match edge {
                        ComboEdge::Down | ComboEdge::Repeat => Some(KeyEvent::Down),
                        ComboEdge::Up => Some(KeyEvent::Up),
                        ComboEdge::Part | ComboEdge::Outside => None,
                    };
                    if let Some(event) = event {
                        let _ = tx.send((event, Instant::now()));
                    }
                }
                for (code, was) in (0u16..).zip(others.iter_mut()) {
                    if keys.contains(&code) {
                        continue;
                    }
                    let down = sys::key_down(code);
                    if down && !*was {
                        if code == KEY_ESCAPE {
                            let _ = tx.send((KeyEvent::Escape, Instant::now()));
                        } else if !is_modifier(code) {
                            let _ = tx.send((KeyEvent::Other, Instant::now()));
                        }
                    }
                    *was = down;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(KeyListener {
        stop,
        thread: Some(thread),
    })
}

/// Waits (up to a second) until the dictation keys are up, so the paste is a
/// plain ⌘V and not ⌥⌘V or ⇧⌘V.
pub fn wait_key_released(codes: &[String]) {
    let keys: Vec<u16> = codes.iter().filter_map(|c| code_key(c)).collect();
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        if !keys.iter().any(|&k| sys::key_down(k)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// ⌘V in the app in front (⇧⌘V when `shift`).
pub fn send_paste(shift: bool) -> Result<(), String> {
    if sys::post_paste(shift) {
        Ok(())
    } else {
        Err("the keystroke could not be made".into())
    }
}

/// The app in front: (bundle id, name). Next Agent itself is never one.
pub fn foreground_app() -> Option<(String, String)> {
    sys::frontmost_app().filter(|(id, _)| id != "dev.nextagent.desktop")
}

/// Apps with a Dock icon, for "Add an app" in Styles.
pub fn running_apps() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for app in sys::running_apps() {
        if app.0 != "dev.nextagent.desktop" && !out.iter().any(|(id, _)| *id == app.0) {
            out.push(app);
        }
    }
    out
}

/// Terminals paste with ⌘V like everything else on a Mac.
pub fn is_terminal(_id: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_code_has_a_mac_key() {
        for code in [
            "ControlRight",
            "AltLeft",
            "MetaLeft",
            "ShiftRight",
            "KeyF",
            "KeyV",
            "Digit7",
            "F12",
            "Space",
        ] {
            assert!(code_key(code).is_some(), "{code}");
        }
        assert_eq!(code_key("KeyV"), Some(9));
        assert_eq!(code_key("Nope"), None);
    }

    #[test]
    fn modifiers_are_never_another_key() {
        for code in [
            "ControlLeft",
            "ControlRight",
            "AltLeft",
            "AltRight",
            "ShiftLeft",
            "ShiftRight",
            "MetaLeft",
            "MetaRight",
        ] {
            assert!(is_modifier(code_key(code).unwrap()), "{code}");
        }
        assert!(!is_modifier(code_key("KeyA").unwrap()));
    }
}
