//! Translates iced key presses into the byte sequences a terminal expects.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

/// Whether this key press is the paste shortcut (Ctrl+Shift+V, or Cmd+V on macOS).
pub fn is_paste(key: &Key, mods: Modifiers) -> bool {
    let v = matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("v"));
    v && ((mods.control() && mods.shift()) || (mods.logo() && !mods.control()))
}

/// Whether this key press is the copy shortcut (Ctrl+Shift+C, or Cmd+C on macOS).
pub fn is_copy(key: &Key, mods: Modifiers) -> bool {
    let c = matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("c"));
    c && ((mods.control() && mods.shift()) || (mods.logo() && !mods.control()))
}

/// Encodes a key press, or `None` when the terminal should ignore it.
pub fn encode(key: &Key, mods: Modifiers, text: Option<&str>, app_cursor: bool) -> Option<Vec<u8>> {
    if let Key::Named(named) = key {
        return named_key(*named, mods, app_cursor);
    }
    if mods.logo() {
        return None;
    }
    if mods.control()
        && let Key::Character(c) = key
        && let Some(byte) = c.chars().next().and_then(control_byte)
    {
        return Some(with_alt(mods, vec![byte]));
    }
    let text = text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control))?;
    Some(with_alt(mods, text.as_bytes().to_vec()))
}

fn with_alt(mods: Modifiers, mut bytes: Vec<u8>) -> Vec<u8> {
    if mods.alt() {
        bytes.insert(0, 0x1b);
    }
    bytes
}

fn control_byte(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '8' | '?' => Some(0x7f),
        _ => None,
    }
}

fn named_key(named: Named, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
    let m = 1 + u8::from(mods.shift()) + 2 * u8::from(mods.alt()) + 4 * u8::from(mods.control());
    let cursor = |c: char| {
        if m > 1 {
            format!("\x1b[1;{m}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| {
        if m > 1 { format!("\x1b[{n};{m}~") } else { format!("\x1b[{n}~") }.into_bytes()
    };
    let bytes = match named {
        Named::Enter => with_alt(mods, b"\r".to_vec()),
        Named::Backspace => with_alt(mods, if mods.control() { vec![0x08] } else { vec![0x7f] }),
        Named::Tab if mods.shift() => b"\x1b[Z".to_vec(),
        Named::Tab => b"\t".to_vec(),
        Named::Escape => b"\x1b".to_vec(),
        Named::Space if mods.control() => vec![0],
        Named::Space => with_alt(mods, b" ".to_vec()),
        Named::ArrowUp => cursor('A'),
        Named::ArrowDown => cursor('B'),
        Named::ArrowRight => cursor('C'),
        Named::ArrowLeft => cursor('D'),
        Named::Home => cursor('H'),
        Named::End => cursor('F'),
        Named::Insert => tilde(2),
        Named::Delete => tilde(3),
        Named::PageUp => tilde(5),
        Named::PageDown => tilde(6),
        Named::F1 => b"\x1bOP".to_vec(),
        Named::F2 => b"\x1bOQ".to_vec(),
        Named::F3 => b"\x1bOR".to_vec(),
        Named::F4 => b"\x1bOS".to_vec(),
        Named::F5 => tilde(15),
        Named::F6 => tilde(17),
        Named::F7 => tilde(18),
        Named::F8 => tilde(19),
        Named::F9 => tilde(20),
        Named::F10 => tilde(21),
        Named::F11 => tilde(23),
        Named::F12 => tilde(24),
        _ => return None,
    };
    Some(bytes)
}

/// Prepares pasted text: newlines become carriage returns and bracketed
/// paste markers are added when the program asked for them.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        // Strip markers from the payload so it cannot end the paste early.
        let clean = normalized.replace("\x1b[201~", "");
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        normalized.into_bytes()
    }
}

/// Ctrl+Z (undo), Ctrl+Shift+Z or Ctrl+Y (redo); Cmd on macOS. `Some(redo)`.
pub fn history_shortcut(key: &Key, mods: Modifiers) -> Option<bool> {
    if !mods.command() {
        return None;
    }
    match key.as_ref() {
        Key::Character(c) if c.eq_ignore_ascii_case("z") => Some(mods.shift()),
        Key::Character(c) if c.eq_ignore_ascii_case("y") && !mods.shift() => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    const NONE: Modifiers = Modifiers::empty();

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(encode(&ch("a"), NONE, Some("a"), false), Some(b"a".to_vec()));
        assert_eq!(encode(&ch("ç"), NONE, Some("ç"), false), Some("ç".as_bytes().to_vec()));
    }

    #[test]
    fn control_and_alt_combinations() {
        assert_eq!(encode(&ch("c"), Modifiers::CTRL, Some("c"), false), Some(vec![3]));
        assert_eq!(encode(&ch("["), Modifiers::CTRL, None, false), Some(vec![0x1b]));
        assert_eq!(encode(&ch("b"), Modifiers::ALT, Some("b"), false), Some(b"\x1bb".to_vec()));
    }

    #[test]
    fn named_keys() {
        let enc = |n, m| encode(&Key::Named(n), m, None, false).unwrap();
        assert_eq!(enc(Named::Enter, NONE), b"\r");
        assert_eq!(enc(Named::Backspace, NONE), b"\x7f");
        assert_eq!(enc(Named::ArrowUp, NONE), b"\x1b[A");
        assert_eq!(enc(Named::ArrowLeft, Modifiers::CTRL), b"\x1b[1;5D");
        assert_eq!(enc(Named::Delete, NONE), b"\x1b[3~");
        assert_eq!(enc(Named::Tab, Modifiers::SHIFT), b"\x1b[Z");
        assert_eq!(encode(&Key::Named(Named::ArrowUp), NONE, None, true).unwrap(), b"\x1bOA");
    }

    #[test]
    fn paste_shortcut_and_payload() {
        assert!(is_paste(&ch("V"), Modifiers::CTRL | Modifiers::SHIFT));
        assert!(!is_paste(&ch("v"), Modifiers::CTRL));
        assert_eq!(paste("a\nb", false), b"a\rb");
        assert_eq!(paste("x\x1b[201~y", true), b"\x1b[200~xy\x1b[201~");
    }

    #[test]
    fn undo_redo_shortcuts() {
        let cmd = if cfg!(target_os = "macos") { Modifiers::LOGO } else { Modifiers::CTRL };
        assert_eq!(history_shortcut(&ch("z"), cmd), Some(false));
        assert_eq!(history_shortcut(&ch("Z"), cmd | Modifiers::SHIFT), Some(true));
        assert_eq!(history_shortcut(&ch("y"), cmd), Some(true));
        assert_eq!(history_shortcut(&ch("z"), Modifiers::empty()), None);
        assert_eq!(history_shortcut(&ch("x"), cmd), None);
    }
}
