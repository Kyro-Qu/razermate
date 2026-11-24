//! Button ids and HID keyboard usages.
//!
//! The button ids are what `get_buttons` reports for the Basilisk V3 — they are
//! *not* sequential and they are *not* USB HID button usages. `forward` is 5 and
//! `back` is 4; sending 90/91 gets you a plain `FAIL` back, which looks like
//! "command not supported" and wastes an afternoon.

/// Physical buttons, as reported by the device.
pub mod button {
    pub const LEFT: u8 = 1;
    pub const RIGHT: u8 = 2;
    pub const MIDDLE: u8 = 3;
    pub const BACK: u8 = 4;
    pub const FORWARD: u8 = 5;
    pub const WHEEL_UP: u8 = 9;
    pub const WHEEL_DOWN: u8 = 10;
    pub const PROFILE: u8 = 14;
    pub const CLUTCH: u8 = 15;
    pub const TILT_LEFT: u8 = 52;
    pub const TILT_RIGHT: u8 = 53;
    pub const DPI: u8 = 96;
    pub const WHEEL_MODE: u8 = 106;

    /// `(id, canonical name)` in the order the device reports them.
    pub const ALL: &[(u8, &str)] = &[
        (LEFT, "left"),
        (RIGHT, "right"),
        (MIDDLE, "middle"),
        (FORWARD, "forward"),
        (BACK, "back"),
        (TILT_LEFT, "tiltleft"),
        (TILT_RIGHT, "tiltright"),
        (DPI, "dpi"),
        (CLUTCH, "clutch"),
        (PROFILE, "profile"),
        (WHEEL_MODE, "wheelmode"),
        (WHEEL_UP, "wheelup"),
        (WHEEL_DOWN, "wheeldown"),
    ];

    /// Accept an id or a name, case-insensitively.
    pub fn parse(name: &str) -> Option<u8> {
        let want = name.trim();
        if let Ok(id) = want.parse::<u8>() {
            return ALL.iter().find(|(i, _)| *i == id).map(|(i, _)| *i);
        }
        let lower = want.to_ascii_lowercase();
        ALL.iter()
            .find(|(_, n)| *n == lower)
            .map(|(i, _)| *i)
            .or_else(|| match lower.as_str() {
                // synonyms people actually type
                "mouse4" | "b4" => Some(FORWARD),
                "mouse5" | "b5" => Some(BACK),
                "scroll" | "wheelclick" => Some(MIDDLE),
                "sniper" | "snipe" => Some(CLUTCH),
                _ => None,
            })
    }

    pub fn name(id: u8) -> &'static str {
        ALL.iter()
            .find(|(i, _)| *i == id)
            .map(|(_, n)| *n)
            .unwrap_or("unknown")
    }
}

/// HID keyboard usage ids (USB HID Usage Tables, chapter 10).
pub mod usage {
    pub const A: u8 = 0x04;
    pub const Z: u8 = 0x1D;
    pub const N1: u8 = 0x1E;
    pub const N0: u8 = 0x27;
    pub const ENTER: u8 = 0x28;
    pub const ESC: u8 = 0x29;
    pub const BACKSPACE: u8 = 0x2A;
    pub const TAB: u8 = 0x2B;
    pub const SPACE: u8 = 0x2C;
    pub const MINUS: u8 = 0x2D;
    pub const EQUALS: u8 = 0x2E;
    pub const LEFT_BRACKET: u8 = 0x2F;
    pub const RIGHT_BRACKET: u8 = 0x30;
    pub const BACKSLASH: u8 = 0x31;
    pub const SEMICOLON: u8 = 0x33;
    pub const QUOTE: u8 = 0x34;
    pub const GRAVE: u8 = 0x35;
    pub const COMMA: u8 = 0x36;
    pub const PERIOD: u8 = 0x37;
    pub const SLASH: u8 = 0x38;
    pub const CAPS_LOCK: u8 = 0x39;
    pub const F1: u8 = 0x3A;
    pub const F12: u8 = 0x45;
    pub const F13: u8 = 0x68;
    pub const PRINT_SCREEN: u8 = 0x46;
    pub const SCROLL_LOCK: u8 = 0x47;
    pub const PAUSE: u8 = 0x48;
    pub const INSERT: u8 = 0x49;
    pub const HOME: u8 = 0x4A;
    pub const PAGE_UP: u8 = 0x4B;
    pub const DELETE: u8 = 0x4C;
    pub const END: u8 = 0x4D;
    pub const PAGE_DOWN: u8 = 0x4E;
    pub const RIGHT: u8 = 0x4F;
    pub const LEFT: u8 = 0x50;
    pub const DOWN: u8 = 0x51;
    pub const UP: u8 = 0x52;

    pub const LCTRL: u8 = 0xE0;
    pub const LSHIFT: u8 = 0xE1;
    pub const LALT: u8 = 0xE2;
    pub const LWIN: u8 = 0xE3;
    pub const RCTRL: u8 = 0xE4;
    pub const RSHIFT: u8 = 0xE5;
    pub const RALT: u8 = 0xE6;
    pub const RWIN: u8 = 0xE7;

    /// Named keys usable in a combo string.
    pub const NAMED: &[(&str, u8)] = &[
        ("ctrl", LCTRL),
        ("lctrl", LCTRL),
        ("rctrl", RCTRL),
        ("shift", LSHIFT),
        ("lshift", LSHIFT),
        ("rshift", RSHIFT),
        ("alt", LALT),
        ("lalt", LALT),
        ("ralt", RALT),
        ("win", LWIN),
        ("lwin", LWIN),
        ("rwin", RWIN),
        ("enter", ENTER),
        ("return", ENTER),
        ("esc", ESC),
        ("escape", ESC),
        ("backspace", BACKSPACE),
        ("tab", TAB),
        ("space", SPACE),
        ("caps", CAPS_LOCK),
        ("capslock", CAPS_LOCK),
        ("minus", MINUS),
        ("equals", EQUALS),
        ("comma", COMMA),
        ("period", PERIOD),
        ("slash", SLASH),
        ("semicolon", SEMICOLON),
        ("quote", QUOTE),
        ("grave", GRAVE),
        ("leftbracket", LEFT_BRACKET),
        ("rightbracket", RIGHT_BRACKET),
        ("backslash", BACKSLASH),
        ("insert", INSERT),
        ("home", HOME),
        ("pageup", PAGE_UP),
        ("pagedown", PAGE_DOWN),
        ("delete", DELETE),
        ("del", DELETE),
        ("end", END),
        ("right", RIGHT),
        ("left", LEFT),
        ("down", DOWN),
        ("up", UP),
        ("printscreen", PRINT_SCREEN),
        ("scrolllock", SCROLL_LOCK),
        ("pause", PAUSE),
        ("f1", F1),
        ("f2", F1 + 1),
        ("f3", F1 + 2),
        ("f4", F1 + 3),
        ("f5", F1 + 4),
        ("f6", F1 + 5),
        ("f7", F1 + 6),
        ("f8", F1 + 7),
        ("f9", F1 + 8),
        ("f10", F1 + 9),
        ("f11", F1 + 10),
        ("f12", F12),
        ("f13", F13),
        ("f14", 0x69),
        ("f15", 0x6A),
        ("f16", 0x6B),
        ("f17", 0x6C),
        ("f18", 0x6D),
        ("f19", 0x6E),
        ("f20", 0x6F),
        ("f21", 0x70),
        ("f22", 0x71),
        ("f23", 0x72),
        ("f24", 0x73),
    ];

    /// Reverse of `lookup`: a short display name for a usage id.
    ///
    /// `NAMED` only covers the function keys and editing keys, so letters, digits
    /// and the numeric keypad are synthesised here. Unknown ids fall back to hex
    /// rather than being silently renamed to something plausible.
    pub fn label(u: u8) -> String {
        if (A..=Z).contains(&u) {
            return ((b'A' + (u - A)) as char).to_string();
        }
        if (N1..=N0).contains(&u) {
            return ((b'1' + (u - N1)) as char).to_string();
        }
        // HID 0x54..=0x63 is the numeric keypad block.
        if (0x54..=0x63).contains(&u) {
            return match u {
                0x54 => "Num /".into(),
                0x55 => "Num *".into(),
                0x56 => "Num -".into(),
                0x57 => "Num +".into(),
                0x58 => "Num Enter".into(),
                0x62 => "Num 0".into(),
                0x63 => "Num .".into(),
                n => format!("Num {}", n - 0x59 + 1),
            };
        }
        match u {
            ENTER => "Enter".into(),
            ESC => "Esc".into(),
            BACKSPACE => "Backspace".into(),
            TAB => "Tab".into(),
            SPACE => "Space".into(),
            CAPS_LOCK => "CapsLock".into(),
            MINUS => "-".into(),
            EQUALS => "=".into(),
            COMMA => ",".into(),
            PERIOD => ".".into(),
            SLASH => "/".into(),
            SEMICOLON => ";".into(),
            QUOTE => "'".into(),
            GRAVE => "`".into(),
            LEFT_BRACKET => "[".into(),
            RIGHT_BRACKET => "]".into(),
            BACKSLASH => "\\".into(),
            INSERT => "Insert".into(),
            HOME => "Home".into(),
            PAGE_UP => "PageUp".into(),
            PAGE_DOWN => "PageDown".into(),
            DELETE => "Delete".into(),
            END => "End".into(),
            RIGHT => "→".into(),
            LEFT => "←".into(),
            DOWN => "↓".into(),
            UP => "↑".into(),
            PRINT_SCREEN => "PrintScreen".into(),
            SCROLL_LOCK => "ScrollLock".into(),
            PAUSE => "Pause".into(),
            LCTRL => "LCtrl".into(),
            LSHIFT => "LShift".into(),
            LALT => "LAlt".into(),
            LWIN => "LWin".into(),
            RCTRL => "RCtrl".into(),
            RSHIFT => "RShift".into(),
            RALT => "RAlt".into(),
            RWIN => "RWin".into(),
            F1..=F12 => format!("F{}", u - F1 + 1),
            0x68..=0x73 => format!("F{}", u - 0x68 + 13),
            other => format!("0x{other:02X}"),
        }
    }

    /// Resolve one token of a combo string to a usage id.
    pub fn lookup(token: &str) -> Option<u8> {
        let t = token.trim().to_ascii_lowercase();
        if t.len() == 1 {
            let c = t.as_bytes()[0];
            if c.is_ascii_lowercase() {
                return Some(A + (c - b'a'));
            }
            if c.is_ascii_digit() {
                return Some(if c == b'0' { N0 } else { N1 + (c - b'1') });
            }
        }
        if let Some(u) = NAMED.iter().find(|(n, _)| *n == t).map(|(_, u)| *u) {
            return Some(u);
        }
        // 最后一招：用展示名反查。这样 `label()` 打出来的 "num enter"、"f13"
        // 都能原样喂回 `lookup`，保证「读出来 → 改回去」不掉信息。
        (0u8..=255).find(|u| label(*u).to_ascii_lowercase() == t)
    }

    /// Usage id for a printable ASCII character (press-and-release pair).
    pub fn for_char(c: char) -> Option<u8> {
        match c {
            'a'..='z' => Some(A + (c as u8 - b'a')),
            'A'..='Z' => Some(A + (c as u8 - b'A')),
            '1'..='9' => Some(N1 + (c as u8 - b'1')),
            '0' => Some(N0),
            ' ' => Some(SPACE),
            '-' => Some(MINUS),
            '=' => Some(EQUALS),
            '[' => Some(LEFT_BRACKET),
            ']' => Some(RIGHT_BRACKET),
            '\\' => Some(BACKSLASH),
            ';' => Some(SEMICOLON),
            '\'' => Some(QUOTE),
            '`' => Some(GRAVE),
            ',' => Some(COMMA),
            '.' => Some(PERIOD),
            '/' => Some(SLASH),
            _ => None,
        }
    }

    /// Characters that need shift, mapped to their unshifted usage.
    pub fn shifted(c: char) -> Option<u8> {
        match c {
            '!' => Some(N1),
            '@' => Some(N1 + 1),
            '#' => Some(N1 + 2),
            '$' => Some(N1 + 3),
            '%' => Some(N1 + 4),
            '^' => Some(N1 + 5),
            '&' => Some(N1 + 6),
            '*' => Some(N1 + 7),
            '(' => Some(N1 + 8),
            ')' => Some(N1 + 9),
            '_' => Some(MINUS),
            '+' => Some(EQUALS),
            '{' => Some(LEFT_BRACKET),
            '}' => Some(RIGHT_BRACKET),
            '|' => Some(BACKSLASH),
            ':' => Some(SEMICOLON),
            '"' => Some(QUOTE),
            '~' => Some(GRAVE),
            '<' => Some(COMMA),
            '>' => Some(PERIOD),
            '?' => Some(SLASH),
            _ => None,
        }
    }
}

/// USB HID consumer-page codes used by `media:` macro steps.
pub mod consumer {
    pub const PLAY_PAUSE: u16 = 0x00CD;
    pub const NEXT_TRACK: u16 = 0x00B5;
    pub const PREV_TRACK: u16 = 0x00B6;
    pub const STOP: u16 = 0x00B7;
    pub const MUTE: u16 = 0x00E2;
    pub const VOLUME_UP: u16 = 0x00E9;
    pub const VOLUME_DOWN: u16 = 0x00EA;

    pub fn lookup(name: &str) -> Option<u16> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "play" | "playpause" | "play_pause" => PLAY_PAUSE,
            "next" | "nexttrack" => NEXT_TRACK,
            "prev" | "prevtrack" => PREV_TRACK,
            "stop" => STOP,
            "mute" => MUTE,
            "volup" | "volumeup" => VOLUME_UP,
            "voldown" | "volumedown" => VOLUME_DOWN,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_and_back_are_not_the_obvious_numbers() {
        assert_eq!(button::FORWARD, 5);
        assert_eq!(button::BACK, 4);
        assert_eq!(button::parse("forward"), Some(5));
        assert_eq!(button::parse("back"), Some(4));
        assert_eq!(button::parse("mouse4"), Some(5));
        assert_eq!(button::parse("mouse5"), Some(4));
        // 90 is not a button; it must not silently pass through.
        assert_eq!(button::parse("90"), None);
    }

    #[test]
    fn no_duplicate_button_ids() {
        let mut ids: Vec<u8> = button::ALL.iter().map(|(i, _)| *i).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate button id in table");
    }

    #[test]
    fn letters_map_to_usage_range() {
        assert_eq!(usage::for_char('a'), Some(0x04));
        assert_eq!(usage::for_char('z'), Some(0x1D));
        assert_eq!(usage::for_char('0'), Some(0x27));
        assert_eq!(usage::for_char('1'), Some(0x1E));
        assert_eq!(usage::for_char(' '), Some(0x2C));
        assert_eq!(usage::for_char('中'), None);
    }

    #[test]
    fn shifted_chars_resolve_to_base_key() {
        assert_eq!(usage::shifted('!'), Some(usage::N1));
        assert_eq!(usage::shifted('?'), Some(usage::SLASH));
    }

    #[test]
    fn named_lookup_is_case_insensitive() {
        assert_eq!(usage::lookup("CTRL"), Some(usage::LCTRL));
        assert_eq!(usage::lookup("F5"), Some(0x3E));
        assert_eq!(usage::lookup("f13"), Some(0x68));
        assert_eq!(usage::lookup("nonsense"), None);
    }
}
