//! Command builders and reply parsers.
//!
//! Every function returns a `Report` request; nothing here touches the device.

use crate::report::{Report, ARG_LEN};

/// Command classes.
pub mod class {
    pub const DEVICE: u8 = 0x00;
    pub const BUTTON: u8 = 0x02;
    /// 鼠标/传感器参数：DPI 与 DPI 档位。
    ///
    /// 实测：`0x04 0x85` 读回 `[profile, x_hi, x_lo, y_hi, y_lo, 0, 0]`，本机得到
    /// `[04, 0A, 8C, 0A, 8C, 00, 00]` → 2700 DPI，与参考实现 qdrazer 的
    /// `sr_with(0x0485, '>BHHxx', profile)` 一致。
    pub const MOUSE: u8 = 0x04;
    pub const PROFILE: u8 = 0x05;
    pub const FLASH: u8 = 0x06;
    pub const SENSOR: u8 = 0x0B;
    pub const LED: u8 = 0x0F;
}

/// `class::DEVICE` command ids.
pub mod device {
    pub const GET_FIRMWARE: u8 = 0x81;
    pub const GET_SERIAL: u8 = 0x82;
    pub const GET_MODE: u8 = 0x84;
    pub const SET_MODE: u8 = 0x04;
    /// 轮询间隔：`[profile, delay_ms]`。1 → 1000 Hz，2 → 500 Hz，8 → 125 Hz。
    pub const GET_POLL_RATE: u8 = 0x8E;
    pub const SET_POLL_RATE: u8 = 0x0E;
}

/// `class::MOUSE` command ids.
pub mod sensor {
    pub const GET_DPI: u8 = 0x85;
    pub const SET_DPI: u8 = 0x05;
    pub const GET_DPI_STAGES: u8 = 0x86;
    pub const SET_DPI_STAGES: u8 = 0x06;

    /// DPI 应答长度：`[profile, x_hi, x_lo, y_hi, y_lo, pad, pad]`。
    pub const DPI_REPLY_LEN: u8 = 7;
    /// 档位应答长度：`[profile, active, count, 5 × (idx, x_hi, x_lo, y_hi, y_lo)]`。
    pub const STAGES_REPLY_LEN: u8 = 38;
}

/// `class::BUTTON` command ids.
pub mod button {
    pub const GET_BINDING: u8 = 0x8C;
    pub const SET_BINDING: u8 = 0x0C;
    pub const GET_SCROLL_MODE: u8 = 0x94;
    pub const SET_SCROLL_MODE: u8 = 0x14;
    pub const GET_SCROLL_ACCEL: u8 = 0x96;
    pub const SET_SCROLL_ACCEL: u8 = 0x16;
    pub const GET_SMART_REEL: u8 = 0x97;
    pub const SET_SMART_REEL: u8 = 0x17;
}

/// `class::PROFILE` command ids.
pub mod profile {
    pub const COUNT: u8 = 0x8A;
    pub const AVAILABLE: u8 = 0x80;
    pub const LIST: u8 = 0x81;
    pub const GET_ACTIVE: u8 = 0x84;
    pub const SET_ACTIVE: u8 = 0x04;
    pub const NEW: u8 = 0x02;
    pub const DELETE: u8 = 0x02;
}

/// `class::FLASH` command ids.
pub mod flash {
    pub const MACRO_COUNT: u8 = 0x80;
    pub const MACRO_SIZE: u8 = 0x88;
    pub const MACRO_READ: u8 = 0x89;
    pub const MACRO_SET_SIZE: u8 = 0x08;
    pub const MACRO_WRITE: u8 = 0x09;
    pub const MACRO_DELETE: u8 = 0x03;
    pub const USAGE: u8 = 0x8E;
}

/// `class::LED` command ids.
pub mod led {
    pub const GET_EFFECT: u8 = 0x82;
    pub const SET_EFFECT: u8 = 0x02;
    pub const GET_BRIGHTNESS: u8 = 0x84;
    pub const SET_BRIGHTNESS: u8 = 0x04;
}

/// Lighting regions.
pub mod region {
    pub const WHEEL: u8 = 0x01;
    pub const LOGO: u8 = 0x04;
    pub const STRIP: u8 = 0x0A;
    /// Writing to `0x00` covers all three; reading from it is invalid.
    pub const ALL: u8 = 0x00;
    pub const STRIP_LEDS: usize = 11;
}

/// Built-in lighting effects.
pub mod effect {
    pub const STATIC: u8 = 1;
    pub const SPECTRUM: u8 = 3;
    pub const WAVE: u8 = 4;
    /// Per-LED colours; runtime only, never persisted to flash.
    pub const CUSTOM: u8 = 8;
}

/// Button layers.
pub mod layer {
    pub const NORMAL: u8 = 0;
    pub const HYPER_SHIFT: u8 = 1;
}

/// Function ids stored in a binding.
///
/// These are *not* in numeric order of usefulness and the byte layout differs
/// per function; see `fn_size`.
pub mod function {
    pub const OFF: u8 = 0;
    pub const MOUSE: u8 = 1;
    /// A key or chord: data = `[modifier bits, hid usage]`.
    pub const KEY: u8 = 2;
    /// Play a macro once or several times: data = `[id_hi, id_lo, times]`.
    pub const MACRO_TIMES: u8 = 3;
    /// Play a macro while the button is held.
    pub const MACRO_HELD: u8 = 4;
    /// Toggle macro playback.
    pub const MACRO_TOGGLE: u8 = 5;
    pub const DPI: u8 = 6;
    /// Next onboard profile.
    pub const PROFILE: u8 = 7;
    /// Consumer-control key: data = `[code_hi, code_lo]`.
    pub const MEDIA: u8 = 10;
    pub const DOUBLE: u8 = 11;
    pub const HYPERSHIFT: u8 = 12;
    pub const TURBO_KEY: u8 = 0x0D;
    /// 扩展鼠标动作用：data = `[mouse_action]`，长度 1。
    ///
    /// 实测依据：本机滚轮左右倾两个键（按钮 52/53）读回来就是 `fn=0x0E data=[0x68]`
    /// 和 `[0x69]`，而 0x68/0x69 正是 `mouse_action::TILT_LEFT/TILT_RIGHT`。
    /// openrazer 把这个 id 记作 "turbo button" 且长度 3，与实测不符，以实测为准。
    pub const MOUSE_ACTION_EXT: u8 = 0x0E;
    /// Free-spin / clicky switch on the wheel.
    pub const WHEEL_MODE: u8 = 0x12;

    /// Payload length for each function id.
    pub fn size(fn_id: u8) -> u8 {
        match fn_id {
            OFF => 0,
            MOUSE | MOUSE_ACTION_EXT | DPI | DOUBLE | HYPERSHIFT | PROFILE | WHEEL_MODE => 1,
            KEY | MEDIA => 2,
            MACRO_TIMES | MACRO_HELD | MACRO_TOGGLE | TURBO_KEY => 3,
            _ => 5,
        }
    }

    /// `profile:next` argument.
    pub const PROFILE_NEXT: u8 = 0x04;
}

/// Modifier bits used by `function::KEY`.
pub mod modifier {
    pub const CTRL: u8 = 0x01;
    pub const SHIFT: u8 = 0x02;
    pub const ALT: u8 = 0x04;
    pub const WIN: u8 = 0x08;
    pub const RCTRL: u8 = 0x10;
    pub const RSHIFT: u8 = 0x20;
    pub const RALT: u8 = 0x40;
    pub const RWIN: u8 = 0x80;

    pub fn bit_for(right_handed: bool, base: u8) -> u8 {
        match (base, right_handed) {
            (CTRL, false) => CTRL,
            (CTRL, true) => RCTRL,
            (SHIFT, false) => SHIFT,
            (SHIFT, true) => RSHIFT,
            (ALT, false) => ALT,
            (ALT, true) => RALT,
            (WIN, false) => WIN,
            (WIN, true) => RWIN,
            _ => 0,
        }
    }
}

/// `function::MOUSE` payload codes. Note these differ from the button *ids*
/// used to address a binding (tilt is 0x68 here, 52 as a button).
pub mod mouse_action {
    pub const LEFT: u8 = 1;
    pub const RIGHT: u8 = 2;
    pub const MIDDLE: u8 = 3;
    pub const BACK: u8 = 4;
    pub const FORWARD: u8 = 5;
    pub const WHEEL_UP: u8 = 9;
    pub const WHEEL_DOWN: u8 = 10;
    pub const TILT_LEFT: u8 = 0x68;
    pub const TILT_RIGHT: u8 = 0x69;
}

/// `function::DPI` payload codes.
pub mod dpi_action {
    pub const UP: u8 = 1;
    pub const DOWN: u8 = 2;
    pub const CYCLE: u8 = 6;
    /// Sentinel meaning "clutch to a fixed dpi"; the value follows in 4 bytes.
    pub const CLUTCH: u8 = 0x08;
}

// ---------------------------------------------------------------------------
// device
// ---------------------------------------------------------------------------

pub fn get_firmware() -> Report {
    Report::request(class::DEVICE, device::GET_FIRMWARE, 4, &[])
}

pub fn get_serial() -> Report {
    Report::request(class::DEVICE, device::GET_SERIAL, 16, &[])
}

pub fn get_mode() -> Report {
    Report::request(class::DEVICE, device::GET_MODE, 2, &[])
}

/// `mode`: 0 normal, 1 bootloader, 2 test, 3 driver.
pub fn set_mode(mode: u8) -> Report {
    Report::request(class::DEVICE, device::SET_MODE, 2, &[mode, 0])
}

/// 读轮询间隔（毫秒）。应答 `[profile, delay_ms]`。
pub fn get_poll_rate(profile: u8) -> Report {
    Report::request(class::DEVICE, device::GET_POLL_RATE, 2, &[profile])
}

pub fn set_poll_rate(profile: u8, delay_ms: u8) -> Report {
    Report::request(class::DEVICE, device::SET_POLL_RATE, 2, &[profile, delay_ms])
}

// ---------------------------------------------------------------------------
// dpi
// ---------------------------------------------------------------------------

pub fn get_dpi(profile: u8) -> Report {
    Report::request(
        class::MOUSE,
        sensor::GET_DPI,
        sensor::DPI_REPLY_LEN,
        &[profile],
    )
}

pub fn set_dpi(profile: u8, x: u16, y: u16) -> Report {
    let [xh, xl] = x.to_be_bytes();
    let [yh, yl] = y.to_be_bytes();
    Report::request(
        class::MOUSE,
        sensor::SET_DPI,
        sensor::DPI_REPLY_LEN,
        &[profile, xh, xl, yh, yl, 0, 0],
    )
}

pub fn get_dpi_stages(profile: u8) -> Report {
    Report::request(
        class::MOUSE,
        sensor::GET_DPI_STAGES,
        sensor::STAGES_REPLY_LEN,
        &[profile],
    )
}

/// 一组 DPI 档位：当前档序号、档数、各档 `(index, x, y)`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiStages {
    pub active: u8,
    pub stages: Vec<(u8, u16, u16)>,
}

impl DpiStages {
    pub fn parse(reply: &[u8]) -> Option<Self> {
        // [profile, active, count, 5 × (idx, x_hi, x_lo, y_hi, y_lo)]
        if reply.len() < 3 {
            return None;
        }
        let active = reply[1];
        let count = (reply[2] as usize).min(5);
        let mut stages = Vec::with_capacity(count);
        for i in 0..count {
            let base = 3 + i * 5;
            let s = reply.get(base..base + 5)?;
            stages.push((
                s[0],
                u16::from_be_bytes([s[1], s[2]]),
                u16::from_be_bytes([s[3], s[4]]),
            ));
        }
        Some(DpiStages { active, stages })
    }
}

/// `(x, y)` in DPI from a `get_dpi` reply.
pub fn parse_dpi(reply: &[u8]) -> Option<(u16, u16)> {
    // [profile, x_hi, x_lo, y_hi, y_lo, pad, pad]
    if reply.len() < 5 {
        return None;
    }
    Some((
        u16::from_be_bytes([reply[1], reply[2]]),
        u16::from_be_bytes([reply[3], reply[4]]),
    ))
}

/// 轮询间隔（毫秒）→ 标称回报率。设备只存间隔，不存 Hz。
pub fn delay_to_hz(delay_ms: u8) -> u16 {
    match delay_ms {
        0 => 0,
        d => 1000 / d as u16,
    }
}

/// 标称回报率 → 轮询间隔。只接受设备实际支持的几档。
pub fn hz_to_delay(hz: u16) -> Option<u8> {
    match hz {
        125 => Some(8),
        500 => Some(2),
        1000 => Some(1),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// buttons
// ---------------------------------------------------------------------------

/// A stored button function: `fn` plus up to five payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub fn_id: u8,
    pub fn_size: u8,
    pub data: [u8; 5],
}

impl Binding {
    pub fn new(fn_id: u8, data: [u8; 5]) -> Self {
        Binding { fn_id, fn_size: function::size(fn_id), data }
    }

    /// A key or chord. `modifiers` is the bit mask, `usage` the main HID key —
    /// the device stores them in that order.
    pub fn key(modifiers: u8, usage: u8) -> Self {
        Binding::new(function::KEY, [modifiers, usage, 0, 0, 0])
    }

    pub fn mouse(code: u8) -> Self {
        Binding::new(function::MOUSE, [code, 0, 0, 0, 0])
    }

    /// Play a macro `times` times per press.
    pub fn macro_play(macro_id: u16, times: u8) -> Self {
        Binding::new(
            function::MACRO_TIMES,
            [(macro_id >> 8) as u8, (macro_id & 0xFF) as u8, times, 0, 0],
        )
    }

    /// 按 `mode` 选触发方式：`MACRO_TIMES` 单次 / `MACRO_HELD` 按住 / `MACRO_TOGGLE` 开关。
    ///
    /// 三种的长度都是 3，差别只在 `fn_id`。设备端是否真的实现了后两种属于固件行为，
    /// 写入能存住不代表按下去会按预期触发，这一点在 PROTOCOL.md 里单独标注。
    pub fn macro_play_with_mode(macro_id: u16, times: u8, mode: u8) -> Self {
        Binding::new(
            mode,
            [(macro_id >> 8) as u8, (macro_id & 0xFF) as u8, times, 0, 0],
        )
    }

    pub fn profile_next() -> Self {
        Binding::new(function::PROFILE, [function::PROFILE_NEXT, 0, 0, 0, 0])
    }

    pub fn wheel_mode() -> Self {
        Binding::new(function::WHEEL_MODE, [1, 0, 0, 0, 0])
    }

    pub fn dpi(action: u8) -> Self {
        Binding::new(function::DPI, [action, 0, 0, 0, 0])
    }

    pub fn media(code: u16) -> Self {
        Binding::new(function::MEDIA, [(code >> 8) as u8, (code & 0xFF) as u8, 0, 0, 0])
    }

    pub fn off() -> Self {
        Binding::new(function::OFF, [0; 5])
    }

    pub fn to_bytes(self) -> [u8; 7] {
        let mut out = [0u8; 7];
        out[0] = self.fn_id;
        out[1] = self.fn_size;
        out[2..7].copy_from_slice(&self.data);
        out
    }

    pub fn from_reply(reply: &[u8]) -> Option<Self> {
        // [profile, button, layer, fn, fn_size, data0..data4]
        if reply.len() < 10 {
            return None;
        }
        let mut data = [0u8; 5];
        data.copy_from_slice(&reply[5..10]);
        Some(Binding { fn_id: reply[3], fn_size: reply[4], data })
    }
}

pub fn get_binding(profile: u8, button: u8, layer: u8) -> Report {
    Report::request(class::BUTTON, button::GET_BINDING, 10, &[profile, button, layer])
}

pub fn set_binding(profile: u8, button: u8, layer: u8, binding: Binding) -> Report {
    let mut args = [0u8; 10];
    args[0] = profile;
    args[1] = button;
    args[2] = layer;
    args[3..10].copy_from_slice(&binding.to_bytes());
    Report::request(class::BUTTON, button::SET_BINDING, 10, &args)
}

pub fn get_scroll_mode(profile: u8) -> Report {
    Report::request(class::BUTTON, button::GET_SCROLL_MODE, 2, &[profile])
}

/// `free_spin = true` means 1 (free), false means 0 (clicky).
pub fn set_scroll_mode(profile: u8, free_spin: bool) -> Report {
    Report::request(
        class::BUTTON,
        button::SET_SCROLL_MODE,
        2,
        &[profile, u8::from(free_spin)],
    )
}

// ---------------------------------------------------------------------------
// profiles
// ---------------------------------------------------------------------------

pub fn profile_count() -> Report {
    Report::request(class::PROFILE, profile::COUNT, 1, &[])
}

pub fn profile_available() -> Report {
    Report::request(class::PROFILE, profile::AVAILABLE, 1, &[])
}

pub fn profile_list() -> Report {
    Report::request(class::PROFILE, profile::LIST, 6, &[])
}

pub fn get_active_profile() -> Report {
    Report::request(class::PROFILE, profile::GET_ACTIVE, 1, &[])
}

pub fn set_active_profile(id: u8) -> Report {
    Report::request(class::PROFILE, profile::SET_ACTIVE, 1, &[id])
}

// ---------------------------------------------------------------------------
// flash / macros
// ---------------------------------------------------------------------------

pub fn macro_count() -> Report {
    Report::request(class::FLASH, flash::MACRO_COUNT, 2, &[])
}

pub fn macro_size(id: u16) -> Report {
    Report::request(class::FLASH, flash::MACRO_SIZE, 6, &id.to_be_bytes())
}

pub fn macro_read(id: u16, offset: u32, len: u8) -> Report {
    let mut args = [0u8; 7];
    args[0..2].copy_from_slice(&id.to_be_bytes());
    args[2..6].copy_from_slice(&offset.to_be_bytes());
    args[6] = len;
    Report::request(class::FLASH, flash::MACRO_READ, 71, &args)
}

pub fn macro_set_size(id: u16, len: u32) -> Report {
    let mut args = [0u8; 6];
    args[0..2].copy_from_slice(&id.to_be_bytes());
    args[2..6].copy_from_slice(&len.to_be_bytes());
    Report::request(class::FLASH, flash::MACRO_SET_SIZE, 6, &args)
}

pub fn macro_write_chunk(id: u16, offset: u32, chunk: &[u8]) -> Report {
    let n = chunk.len().min(64);
    let size = 7 + n;
    let mut args = vec![0u8; size];
    args[0..2].copy_from_slice(&id.to_be_bytes());
    args[2..6].copy_from_slice(&offset.to_be_bytes());
    args[6] = n as u8;
    args[7..7 + n].copy_from_slice(&chunk[..n]);
    Report::request(class::FLASH, flash::MACRO_WRITE, size as u8, &args)
}

pub fn macro_delete(id: u16) -> Report {
    Report::request(class::FLASH, flash::MACRO_DELETE, 2, &id.to_be_bytes())
}

pub fn flash_usage() -> Report {
    Report::request(class::FLASH, flash::USAGE, 14, &[])
}

/// Parsed `flash_usage` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashInfo {
    /// Two leading bytes whose meaning is unknown; kept for round-tripping.
    pub unknown: u16,
    pub total: u32,
    pub available: u32,
    pub recycled: u32,
}

impl FlashInfo {
    pub fn parse(reply: &[u8]) -> Option<Self> {
        if reply.len() < 14 {
            return None;
        }
        Some(FlashInfo {
            unknown: u16::from_be_bytes([reply[0], reply[1]]),
            total: u32::from_be_bytes([reply[2], reply[3], reply[4], reply[5]]),
            available: u32::from_be_bytes([reply[6], reply[7], reply[8], reply[9]]),
            recycled: u32::from_be_bytes([reply[10], reply[11], reply[12], reply[13]]),
        })
    }

    /// `available` already includes `recycled`. This is what can be handed out
    /// right now without a format.
    pub fn immediately_usable(&self) -> u32 {
        self.available.saturating_sub(self.recycled)
    }

    pub fn used(&self) -> u32 {
        self.total.saturating_sub(self.available)
    }

    pub fn page_size(&self) -> u32 {
        256
    }
}

// ---------------------------------------------------------------------------
// lighting
// ---------------------------------------------------------------------------

pub fn get_effect(profile: u8, region_id: u8) -> Report {
    Report::request(class::LED, led::GET_EFFECT, 2, &[profile, region_id])
}

/// One colour per region, or 11 for the strip.
pub fn set_static(profile: u8, region_id: u8, colors: &[(u8, u8, u8)]) -> Report {
    let n = colors.len().min(region::STRIP_LEDS) as u8;
    let mut args = Vec::with_capacity(5 + n as usize * 3);
    args.push(profile);
    args.push(region_id);
    args.push(effect::STATIC);
    args.push(0); // mode
    args.push(0); // speed
    args.push(n);
    for (r, g, b) in colors {
        args.extend_from_slice(&[*r, *g, *b]);
    }
    Report::request(class::LED, led::SET_EFFECT, args.len() as u8, &args)
}

pub fn set_spectrum(profile: u8, region_id: u8) -> Report {
    Report::request(
        class::LED,
        led::SET_EFFECT,
        6,
        &[profile, region_id, effect::SPECTRUM, 1, 0, 0],
    )
}

/// `direction` 1 clockwise, 2 counter-clockwise. `speed` 0 fastest, 255 slowest.
pub fn set_wave(profile: u8, region_id: u8, direction: u8, speed: u8) -> Report {
    Report::request(
        class::LED,
        led::SET_EFFECT,
        6,
        &[profile, region_id, effect::WAVE, direction, speed, 0],
    )
}

/// Per-LED colours, applied immediately and **never** persisted to flash.
///
/// 用途是运行时的动画帧：每帧一颗一条颜色。写多少次都不消耗 Flash 页，
/// 但也意味着断电就没了——想让效果长期生效必须用 `set_spectrum` / `set_wave`。
pub fn set_custom(profile: u8, region_id: u8, colors: &[(u8, u8, u8)]) -> Report {
    let n = colors.len().min(region::STRIP_LEDS) as u8;
    let mut args = Vec::with_capacity(6 + n as usize * 3);
    args.push(profile);
    args.push(region_id);
    args.push(effect::CUSTOM);
    args.push(0); // mode
    args.push(0); // speed
    args.push(n);
    for (r, g, b) in colors {
        args.extend_from_slice(&[*r, *g, *b]);
    }
    Report::request(class::LED, led::SET_EFFECT, args.len() as u8, &args)
}

pub fn get_brightness(profile: u8, region_id: u8) -> Report {
    Report::request(class::LED, led::GET_BRIGHTNESS, 2, &[profile, region_id])
}

pub fn set_brightness(profile: u8, region_id: u8, percent: u8) -> Report {
    Report::request(
        class::LED,
        led::SET_BRIGHTNESS,
        3,
        &[profile, region_id, percent],
    )
}

/// Parsed lighting reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lighting {
    pub effect: u8,
    pub mode: u8,
    pub speed: u8,
    pub colors: Vec<(u8, u8, u8)>,
}

impl Lighting {
    pub fn parse(reply: &[u8]) -> Option<Self> {
        if reply.len() < 6 {
            return None;
        }
        let n = reply[5] as usize;
        // 逐颗（CUSTOM）效果时设备只回报 12 字节，也就是头两颗灯的颜色，
        // 但字段 n 仍写着 11。按实际拿到的字节数取，别因为"声明的比给的多"
        // 就把整条应答丢掉——那会让灯效读起来像 HID 故障。
        let available = (reply.len() - 6) / 3;
        let take = n.min(available);
        let mut colors = Vec::with_capacity(take);
        for i in 0..take {
            let base = 6 + i * 3;
            colors.push((reply[base], reply[base + 1], reply[base + 2]));
        }
        Some(Lighting {
            effect: reply[2],
            mode: reply[3],
            speed: reply[4],
            colors,
        })
    }

    pub fn describe(&self) -> String {
        match self.effect {
            effect::STATIC => {
                if let Some((r, g, b)) = self.colors.first() {
                    format!("static #{r:02x}{g:02x}{b:02x}")
                } else {
                    "static".to_string()
                }
            }
            effect::SPECTRUM => "spectrum".to_string(),
            effect::WAVE => format!(
                "wave dir{} speed{}",
                self.mode, self.speed
            ),
            effect::CUSTOM => format!("custom ({} colours reported)", self.colors.len()),
            other => format!("effect {other}"),
        }
    }
}

/// Split a `data_size`-length reply into the echoed prefix and the payload.
pub fn strip_echo(reply: &[u8], echo_len: usize) -> &[u8] {
    if reply.len() > echo_len {
        &reply[echo_len..]
    } else {
        &[]
    }
}

/// Read a u16 big-endian pair from the start of a reply.
pub fn be_u16(reply: &[u8]) -> Option<u16> {
    if reply.len() < 2 {
        return None;
    }
    Some(u16::from_be_bytes([reply[0], reply[1]]))
}

/// Guard used by tests and the CLI: the argument block never exceeds 80 bytes.
pub fn args_len(report: &Report) -> usize {
    report.data_size as usize % (ARG_LEN + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custom_effect_with_a_short_reply() {
        // 实测：CUSTOM 时设备回 12 字节，n 写 11 但只给得出 2 颗颜色。
        // 旧实现要求 6 + n*3 字节，整条应答被丢掉，上层误报成 HID 故障。
        let reply = [
            0u8, 0x0a, effect::CUSTOM, 0, 0, 11,
            0xff, 0x88, 0x00, 0xff, 0xe9, 0x00,
        ];
        let l = Lighting::parse(&reply).expect("short custom reply must still parse");
        assert_eq!(l.effect, effect::CUSTOM);
        assert_eq!(l.colors.len(), 2);
        assert_eq!(l.colors[0], (0xff, 0x88, 0x00));
        assert_eq!(l.colors[1], (0xff, 0xe9, 0x00));
    }

    #[test]
    fn binding_reply_parses_at_the_right_offset() {
        // [profile, button, layer, fn, fn_size, data(5)] — a real forward
        // binding: key, alt held, usage 0x1a (w)
        let reply = [4u8, 5, 0, 0x02, 2, 0x04, 0x1A, 0, 0, 0];
        let b = Binding::from_reply(&reply).unwrap();
        assert_eq!(b.fn_id, function::KEY);
        assert_eq!(b.data[0], 0x04);
        assert_eq!(b.data[1], 0x1A);
    }

    #[test]
    fn key_payload_is_modifiers_then_usage() {
        let b = Binding::key(modifier::ALT, 0x1A);
        assert_eq!(b.to_bytes(), [0x02, 2, 0x04, 0x1A, 0, 0, 0]);
    }

    #[test]
    fn macro_binding_is_big_endian_id_plus_times() {
        let b = Binding::macro_play(0x0101, 1);
        assert_eq!(b.to_bytes(), [0x03, 3, 0x01, 0x01, 1, 0, 0]);
    }

    #[test]
    fn flash_reply_splits_into_four_fields() {
        let reply = [
            0x00, 0x64, // unknown = 100
            0x00, 0x01, 0x68, 0x00, // total = 92160
            0x00, 0x01, 0x61, 0x00, // available = 90368
            0x00, 0x00, 0x09, 0x00, // recycled = 2304
        ];
        let info = FlashInfo::parse(&reply).unwrap();
        assert_eq!(info.unknown, 100);
        assert_eq!(info.total, 92160);
        assert_eq!(info.available, 90368);
        assert_eq!(info.recycled, 2304);
        // available already contains recycled
        assert_eq!(info.immediately_usable(), 88064);
        assert_eq!(info.used(), 1792);
    }

    #[test]
    fn lighting_reply_matches_a_real_read() {
        // real reply for slot 4 strip: blue, static, one colour
        let reply = [4u8, 0x0A, 1, 0, 0, 1, 0x00, 0x00, 0xFF];
        let l = Lighting::parse(&reply).unwrap();
        assert_eq!(l.effect, effect::STATIC);
        assert_eq!(l.colors, vec![(0x00, 0x00, 0xFF)]);
        assert_eq!(l.describe(), "static #0000ff");
    }

    #[test]
    fn static_write_has_the_documented_layout() {
        let r = set_static(4, region::STRIP, &[(0xFF, 0x00, 0x00)]);
        assert_eq!(r.data_size, 9);
        assert_eq!(&r.arguments[..9], &[4, 0x0A, 1, 0, 0, 1, 0xFF, 0x00, 0x00]);
    }

    #[test]
    fn wave_keeps_direction_and_speed() {
        let r = set_wave(1, region::STRIP, 2, 0x60);
        assert_eq!(&r.arguments[..6], &[1, 0x0A, 4, 2, 0x60, 0]);
    }

    #[test]
    fn macro_write_chunk_sets_length_byte() {
        let chunk = [0xAAu8; 10];
        let r = macro_write_chunk(0x0002, 64, &chunk);
        assert_eq!(r.data_size, 17);
        assert_eq!(&r.arguments[..7], &[0x00, 0x02, 0x00, 0x00, 0x00, 0x40, 10]);
        assert_eq!(r.arguments[7], 0xAA);
    }

    #[test]
    fn active_profile_request_is_one_byte() {
        let r = set_active_profile(3);
        assert_eq!(r.command_id, profile::SET_ACTIVE);
        assert_eq!(r.data_size, 1);
        assert_eq!(r.arguments[0], 3);
    }
}
