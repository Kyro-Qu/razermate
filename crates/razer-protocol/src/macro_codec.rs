//! Macro body codec.
//!
//! A macro body is a flat byte stream of pressed/released events. Timing is
//! carried by the delay opcodes, not by the transport, so a macro replays at
//! device speed once it lives in flash.

use crate::keys::{consumer, usage};

pub const OP_PRESS: u8 = 0x01;
pub const OP_RELEASE: u8 = 0x02;
pub const OP_MOUSE: u8 = 0x08;
pub const OP_WHEEL: u8 = 0x0A;
pub const OP_WAIT: u8 = 0x11;
pub const OP_WAIT_LONG: u8 = 0x12;
pub const OP_MEDIA_DOWN: u8 = 0x05;
pub const OP_MEDIA_UP: u8 = 0x06;

/// One step of a human-written macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A key or combo, e.g. `ctrl+shift+r`.
    Combo(String),
    /// Literal ASCII text, typed one character at a time.
    Text(String),
    /// Delay in milliseconds.
    Wait(u32),
    /// Mouse button click.
    Click(MouseButton),
    /// Wheel notches, positive is up.
    Wheel(i8),
    /// Consumer-control key.
    Media(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    fn mask(self) -> u8 {
        match self {
            MouseButton::Left => 0x01,
            MouseButton::Right => 0x02,
            MouseButton::Middle => 0x04,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "left" | "l" | "1" => MouseButton::Left,
            "right" | "r" | "2" => MouseButton::Right,
            "middle" | "m" | "3" => MouseButton::Middle,
            _ => return None,
        })
    }
}

/// Parse one textual step: `text:hi`, `ctrl+c`, `wait:120`, `click:left`,
/// `wheel:-1`, `media:play`.
pub fn parse_step(raw: &str) -> Result<Step, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty step".into());
    }
    let lower = s.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("text:").or_else(|| lower.strip_prefix("type:")) {
        // Keep the original casing of the payload.
        let payload = &s[s.len() - rest.len()..];
        return Ok(Step::Text(payload.to_string()));
    }
    if let Some(rest) = lower.strip_prefix("wait:").or_else(|| lower.strip_prefix("delay:")) {
        let digits = rest.trim_end_matches("ms").trim();
        let ms: u32 = digits
            .parse()
            .map_err(|_| format!("bad delay {raw:?}, expected milliseconds"))?;
        if ms == 0 || ms > 0xFFFF {
            return Err(format!("delay {ms}ms out of range 1..=65535"));
        }
        return Ok(Step::Wait(ms));
    }
    if let Some(rest) = lower.strip_prefix("click:") {
        return MouseButton::parse(rest)
            .map(Step::Click)
            .ok_or_else(|| format!("unknown mouse button {rest:?}"));
    }
    if let Some(rest) = lower.strip_prefix("wheel:").or_else(|| lower.strip_prefix("scroll:")) {
        let delta: i8 = rest
            .trim()
            .parse()
            .map_err(|_| format!("bad wheel delta {rest:?}"))?;
        return Ok(Step::Wheel(delta));
    }
    if let Some(rest) = lower.strip_prefix("media:") {
        if consumer::lookup(rest).is_none() {
            return Err(format!("unknown media key {rest:?}"));
        }
        return Ok(Step::Media(rest.trim().to_string()));
    }
    Ok(Step::Combo(s.to_string()))
}

/// Encode a combo such as `ctrl+shift+r`.
///
/// Order matters: modifiers go down first, the main key next, then release in
/// reverse order. The device replays exactly what is written.
pub fn encode_combo(combo: &str) -> Result<Vec<u8>, String> {
    let mut modifiers: Vec<u8> = Vec::new();
    let mut main: Option<u8> = None;

    for token in combo.split('+').map(str::trim).filter(|t| !t.is_empty()) {
        let usage = usage::lookup(token).ok_or_else(|| format!("unknown key {token:?}"))?;
        if (usage::LCTRL..=usage::RWIN).contains(&usage) {
            modifiers.push(usage);
        } else if main.is_none() {
            main = Some(usage);
        } else {
            return Err(format!("combo {combo:?} has more than one main key"));
        }
    }

    let main = main.ok_or_else(|| format!("combo {combo:?} has no main key"))?;

    let mut out = Vec::with_capacity(modifiers.len() * 4 + 4);
    for m in &modifiers {
        out.extend_from_slice(&[OP_PRESS, *m]);
    }
    out.extend_from_slice(&[OP_PRESS, main, OP_RELEASE, main]);
    for m in modifiers.iter().rev() {
        out.extend_from_slice(&[OP_RELEASE, *m]);
    }
    Ok(out)
}

/// Encode one character, adding shift when the character needs it.
pub fn encode_char(c: char) -> Result<Vec<u8>, String> {
    if let Some(u) = usage::for_char(c) {
        let needs_shift = c.is_ascii_uppercase();
        return Ok(if needs_shift {
            vec![OP_PRESS, usage::LSHIFT, OP_PRESS, u, OP_RELEASE, u, OP_RELEASE, usage::LSHIFT]
        } else {
            vec![OP_PRESS, u, OP_RELEASE, u]
        });
    }
    if let Some(u) = usage::shifted(c) {
        return Ok(vec![
            OP_PRESS, usage::LSHIFT, OP_PRESS, u, OP_RELEASE, u, OP_RELEASE, usage::LSHIFT,
        ]);
    }
    Err(format!("character {c:?} has no HID usage (only ASCII is possible)"))
}

pub fn encode_step(step: &Step) -> Result<Vec<u8>, String> {
    match step {
        Step::Combo(c) => encode_combo(c),
        Step::Text(t) => {
            let mut out = Vec::with_capacity(t.len() * 4);
            for c in t.chars() {
                out.extend(encode_char(c)?);
            }
            Ok(out)
        }
        Step::Wait(ms) => {
            if *ms <= 0xFF {
                Ok(vec![OP_WAIT, *ms as u8])
            } else {
                Ok(vec![OP_WAIT_LONG, (*ms >> 8) as u8, (*ms & 0xFF) as u8])
            }
        }
        Step::Click(b) => Ok(vec![OP_MOUSE, b.mask(), OP_MOUSE, 0x00]),
        Step::Wheel(d) => Ok(vec![OP_WHEEL, *d as u8]),
        Step::Media(name) => {
            let code = consumer::lookup(name).ok_or_else(|| format!("unknown media key {name:?}"))?;
            Ok(vec![
                OP_MEDIA_DOWN,
                (code >> 8) as u8,
                (code & 0xFF) as u8,
                OP_MEDIA_UP,
                0x00,
                0x00,
            ])
        }
    }
}

pub fn encode_steps(steps: &[Step]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for s in steps {
        out.extend(encode_step(s)?);
    }
    Ok(out)
}

/// Parse a comma-separated step list the way the CLI takes it.
pub fn parse_steps(spec: &str) -> Result<Vec<Step>, String> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_step)
        .collect()
}

/// 把字节流还原成结构化步骤。
///
/// 遇到无法用 `Step` 表达的东西（半按 `+X` / 半放 `-X`、未知操作码）就返回 `None`。
/// 这一点是故意的：编辑器拿不到完整语义时宁可拒绝，也不能"尽力而为"地重编码——
/// 一保存就会把用户原本的宏改成别的东西。
pub fn decode_steps(data: &[u8]) -> Option<Vec<Step>> {
    let mut out: Vec<Step> = Vec::new();
    let mut pending_mods: Vec<u8> = Vec::new();
    let mut i = 0usize;

    while i < data.len() {
        let op = data[i];
        match op {
            OP_PRESS => {
                let u = *data.get(i + 1)?;
                if (usage::LCTRL..=usage::RWIN).contains(&u) {
                    pending_mods.push(u);
                    i += 2;
                    continue;
                }
                if data.get(i + 2) == Some(&OP_RELEASE) && data.get(i + 3) == Some(&u) {
                    let mut combo: Vec<String> =
                        pending_mods.iter().map(|m| usage_name(*m)).collect();
                    combo.push(usage_name(u));
                    pending_mods.clear();
                    out.push(Step::Combo(combo.join("+")));
                    i += 4;
                    continue;
                }
                // 按下但没有立刻抬起：这是"按住"语义，Step 表达不了。
                return None;
            }
            OP_RELEASE => {
                // 组合键的修饰键是在主键之后才抬起的，这里要把它们放掉；
                // 非修饰键的孤立抬起说明是不成对的，Step 表达不了。
                let u = *data.get(i + 1)?;
                if !(usage::LCTRL..=usage::RWIN).contains(&u) {
                    return None;
                }
                pending_mods.retain(|m| *m != u);
                i += 2;
            }
            OP_MOUSE => {
                let mask = *data.get(i + 1)?;
                if mask == 0 {
                    // 点击是"按下 + 抬起"两条：抬起那条吃掉即可，但要确认前面
                    // 真的有一次点击，否则就是个孤立的抬起，不能装作没看见。
                    if matches!(out.last(), Some(Step::Click(_))) {
                        i += 2;
                        continue;
                    }
                    return None;
                }
                let button = match mask {
                    0x01 => MouseButton::Left,
                    0x02 => MouseButton::Right,
                    0x04 => MouseButton::Middle,
                    _ => return None,
                };
                out.push(Step::Click(button));
                i += 2;
            }
            OP_WHEEL => {
                out.push(Step::Wheel(*data.get(i + 1)? as i8));
                i += 2;
            }
            OP_WAIT => {
                out.push(Step::Wait(*data.get(i + 1)? as u32));
                i += 2;
            }
            OP_WAIT_LONG => {
                let ms = u16::from_be_bytes([*data.get(i + 1)?, *data.get(i + 2)?]);
                out.push(Step::Wait(ms as u32));
                i += 3;
            }
            OP_MEDIA_DOWN => {
                let code = u16::from_be_bytes([*data.get(i + 1)?, *data.get(i + 2)?]);
                out.push(Step::Media(media_name(code)));
                // 抬起事件跟着一起来，验证一下再跳过。
                if data.get(i + 3) == Some(&OP_MEDIA_UP) {
                    i += 6;
                } else {
                    i += 3;
                }
            }
            _ => return None,
        }
    }
    if pending_mods.is_empty() {
        Some(out)
    } else {
        None
    }
}

/// Render a macro body back to readable text.
pub fn decode(data: &[u8]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut pending_mods: Vec<u8> = Vec::new();
    let mut i = 0usize;

    while i < data.len() {
        let op = data[i];
        match op {
            OP_PRESS => {
                if i + 1 >= data.len() {
                    parts.push(format!("?{:02x}", op));
                    break;
                }
                let u = data[i + 1];
                if (usage::LCTRL..=usage::RWIN).contains(&u) {
                    pending_mods.push(u);
                } else {
                    // A press followed by its own release is a plain keystroke.
                    if data.get(i + 2) == Some(&OP_RELEASE) && data.get(i + 3) == Some(&u) {
                        let main = usage_name(u);
                        if pending_mods.is_empty() {
                            parts.push(main);
                        } else {
                            let mut combo: Vec<String> =
                                pending_mods.iter().map(|m| usage_name(*m)).collect();
                            combo.push(main);
                            parts.push(combo.join("+"));
                            pending_mods.clear();
                        }
                        i += 4;
                        continue;
                    }
                    parts.push(format!("+{}", usage_name(u)));
                }
                i += 2;
            }
            OP_RELEASE => {
                if i + 1 >= data.len() {
                    break;
                }
                let u = data[i + 1];
                if (usage::LCTRL..=usage::RWIN).contains(&u) {
                    pending_mods.retain(|m| *m != u);
                } else {
                    parts.push(format!("-{}", usage_name(u)));
                }
                i += 2;
            }
            OP_MOUSE => {
                let mask = data.get(i + 1).copied().unwrap_or(0);
                let label = match mask {
                    0x01 => "click:left",
                    0x02 => "click:right",
                    0x04 => "click:middle",
                    0x00 => "",
                    _ => "click:?",
                };
                if !label.is_empty() {
                    parts.push(label.to_string());
                }
                i += 2;
            }
            OP_WHEEL => {
                if let Some(d) = data.get(i + 1) {
                    parts.push(format!("wheel:{}", *d as i8));
                }
                i += 2;
            }
            OP_WAIT => {
                if let Some(ms) = data.get(i + 1) {
                    parts.push(format!("wait:{ms}"));
                }
                i += 2;
            }
            OP_WAIT_LONG => {
                if i + 2 < data.len() {
                    let ms = u16::from_be_bytes([data[i + 1], data[i + 2]]);
                    parts.push(format!("wait:{ms}"));
                }
                i += 3;
            }
            OP_MEDIA_DOWN => {
                if i + 2 < data.len() {
                    let code = u16::from_be_bytes([data[i + 1], data[i + 2]]);
                    parts.push(format!("media:{}", media_name(code)));
                }
                i += 3;
            }
            OP_MEDIA_UP => i += 3,
            other => {
                parts.push(format!("?{:02x}", other));
                i += 1;
            }
        }
    }
    parts.join(" ")
}

fn usage_name(u: u8) -> String {
    if let Some((n, _)) = usage::NAMED.iter().find(|(_, v)| *v == u) {
        return (*n).to_string();
    }
    if (usage::A..=usage::Z).contains(&u) {
        return ((b'a' + (u - usage::A)) as char).to_string();
    }
    if (usage::N1..=usage::N0).contains(&u) {
        let n = u - usage::N1;
        return if n == 9 { "0".into() } else { ((b'1' + n) as char).to_string() };
    }
    format!("u{:02x}", u)
}

fn media_name(code: u16) -> String {
    let table = [
        (consumer::PLAY_PAUSE, "play"),
        (consumer::NEXT_TRACK, "next"),
        (consumer::PREV_TRACK, "prev"),
        (consumer::STOP, "stop"),
        (consumer::MUTE, "mute"),
        (consumer::VOLUME_UP, "volup"),
        (consumer::VOLUME_DOWN, "voldown"),
    ];
    table
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| (*n).to_string())
        .unwrap_or_else(|| format!("0x{code:04x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_puts_modifiers_down_first_and_up_last() {
        let body = encode_combo("ctrl+shift+r").unwrap();
        assert_eq!(body, vec![0x01, 0xE0, 0x01, 0xE1, 0x01, 0x15, 0x02, 0x15, 0x02, 0xE1, 0x02, 0xE0]);
    }

    #[test]
    fn text_encodes_each_char() {
        let body = encode_step(&Step::Text("ok".into())).unwrap();
        // o = 0x12, k = 0x0E
        assert_eq!(body, vec![0x01, 0x12, 0x02, 0x12, 0x01, 0x0E, 0x02, 0x0E]);
    }

    #[test]
    fn uppercase_needs_shift() {
        let body = encode_char('A').unwrap();
        assert_eq!(body, vec![0x01, 0xE1, 0x01, 0x04, 0x02, 0x04, 0x02, 0xE1]);
    }

    #[test]
    fn wait_picks_short_or_long_form() {
        assert_eq!(encode_step(&Step::Wait(120)).unwrap(), vec![0x11, 120]);
        assert_eq!(encode_step(&Step::Wait(500)).unwrap(), vec![0x12, 0x01, 0xF4]);
    }

    #[test]
    fn media_is_press_release_pair() {
        let body = encode_step(&Step::Media("play".into())).unwrap();
        assert_eq!(body, vec![0x05, 0x00, 0xCD, 0x06, 0x00, 0x00]);
    }

    #[test]
    fn decode_restores_combo() {
        let body = encode_combo("ctrl+shift+r").unwrap();
        assert_eq!(decode(&body), "ctrl+shift+r");
    }

    #[test]
    fn decode_restores_plain_typing() {
        let body = encode_step(&Step::Text("ok".into())).unwrap();
        assert_eq!(decode(&body), "o k");
    }

    #[test]
    fn roundtrip_through_parse() {
        let steps = parse_steps("ctrl+shift+r, wait:120, text:ok").unwrap();
        let body = encode_steps(&steps).unwrap();
        let text = decode(&body);
        assert!(text.contains("ctrl+shift+r"), "missing combo: {text}");
        assert!(text.contains("wait:120"), "missing wait: {text}");
        assert!(text.contains("o k"), "missing text: {text}");
    }

    #[test]
    fn chinese_is_rejected_not_silently_dropped() {
        let err = encode_step(&Step::Text("中文".into())).unwrap_err();
        assert!(err.contains("ASCII"), "unexpected message: {err}");
    }

    #[test]
    fn decode_steps_round_trips_what_we_can_express() {
        let steps = parse_steps("ctrl+shift+r, wait:120, click:left, wheel:-1, media:play").unwrap();
        let body = encode_steps(&steps).unwrap();
        let back = decode_steps(&body).unwrap_or_else(|| panic!("不可还原：{:02x?}", body));
        assert_eq!(back, steps);
    }

    #[test]
    fn decode_steps_refuses_half_presses() {
        // 按下不抬起：编辑器不能"猜"它本来是什么，只能拒绝。
        assert!(decode_steps(&[0x01, 0x04]).is_none());
        assert!(decode_steps(&[0x02, 0x04]).is_none());
        assert!(decode_steps(&[0xFF]).is_none());
    }

    #[test]
    fn empty_combo_is_an_error() {
        assert!(parse_step("").is_err());
        assert!(encode_combo("+").is_err());
    }
}
