// RazerMate — Basilisk V3 控制面板
//
// 内存策略（对应 PLAN.md 模式一）：
//   * 只在「需要设备」时才打开 HID 句柄，用完立刻关掉，鼠标不会被独占；
//   * 界面不加载任何图片、不用 GPU 后端，Slint 走 software renderer；
//   * 所有耗时读写放在一次性线程里，主线程只做属性赋值。
//
// 数据策略：界面上显示的每一个数字都必须来自设备实测，读不到就显示「—」，
// 绝不填默认值。
//
// 写入策略：每次写都由 razer-core 做「读旧值 → 写 → 读回校验 → 失败回滚」，
// 界面只负责把结果如实回报给用户。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use razer_core::{
    binding_to_edit, button_label_zh, chord_token, describe_binding_zh, edit_to_spec,
    key_name_from_slint_text, light_detail_zh, light_name_zh, parse_action, slot_label_zh, slot_rgb,
    Mouse,
};
use razer_ipc::{client, paths, rules::Rules, vslots::{Index as VsIndex, SlotConfig, SlotMeta, LedSetting}};
use razer_protocol::commands as cmd;
use razer_protocol::keys::button as btn;
use razer_protocol::macro_codec::{self, MouseButton, Step};

use slint::{Color, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 一次完整采集的结果。所有字段都已经是「能直接贴到界面上的」形态。
struct Payload {
    path: String,
    firmware: String,
    mode_text: String,
    connected: bool,
    status: String,

    active: u8,
    slot_name: String,
    flash_ratio: f32,
    flash_text: String,

    buttons: Vec<ButtonRow>,
    macros: Vec<MacroRow>,
    macro_count: i32,
    macro_bytes: String,

    lights: Vec<LightRow>,
    light_effect: String,
    light_brightness: i32,
    light_rgb: (u8, u8, u8),
    light_region: String,
    strip_bright: i32,

    dpi_text: String,
    polling_text: String,
    stages: Vec<StageRow>,

    slots: Vec<SlotRow>,
}

fn mode_text_zh(mode: u8) -> String {
    // 这是「设备协议层」的状态，不是顶栏那个「运行模式」。
    // 协议 normal(0)=固件执行板载配置，driver(3)=写 DPI 时短暂切入。
    // 用协议本名 + 前缀，避免和「离线/在线运行模式」混淆。
    let inner = match mode {
        0 => "普通 (0)",
        1 => "引导 (1)",
        2 => "测试 (2)",
        3 => "驱动 (3)",
        other => return format!("设备层：未知 ({other})"),
    };
    format!("设备层 · {inner}")
}

fn rgb_of(l: &razer_protocol::commands::Lighting) -> (u8, u8, u8) {
    l.colors.first().copied().unwrap_or((0, 0, 0))
}

fn empty_payload(status: String) -> Payload {
    Payload {
        path: "—".into(),
        firmware: "—".into(),
        mode_text: "—".into(),
        connected: false,
        status,
        active: 0,
        slot_name: "—".into(),
        flash_ratio: 0.0,
        flash_text: "—".into(),
        buttons: Vec::new(),
        macros: Vec::new(),
        macro_count: 0,
        macro_bytes: "—".into(),
        lights: Vec::new(),
        light_effect: "—".into(),
        light_brightness: 0,
        light_rgb: (0x44, 0xd6, 0x2c),
        light_region: "—".into(),
        strip_bright: 100,
        dpi_text: "—".into(),
        polling_text: "—".into(),
        stages: Vec::new(),
        slots: Vec::new(),
    }
}

/// 采集全部界面数据。任何单项失败都降级成占位符，不影响其他项。
fn collect() -> Payload {
    let mouse = match Mouse::open() {
        Ok(m) => m,
        Err(e) => return empty_payload(format!("未找到设备：{e}")),
    };

    let path = mouse.path().to_string();
    // 完整 HID 路径有 100+ 字符，放状态栏只会挤掉真正重要的状态信息。
    // GUI 只显示 VID / PID / 控制口，完整路径仍可用 razerctl info 查。
    let lower = path.to_ascii_lowercase();
    let part = |key: &str, n: usize, fallback: &str| -> String {
        lower.split(key).nth(1).and_then(|s| s.get(..n)).unwrap_or(fallback).to_string()
    };
    let path_display = format!(
        "USB HID · {}:{} · 控制口 MI_{}",
        part("vid_", 4, "1532"),
        part("pid_", 4, "0099"),
        part("mi_", 2, "03")
    );

    let firmware = match mouse.firmware() {
        Ok((a, b, c)) => format!("{a}.{b}.{c}"),
        Err(_) => "—".into(),
    };

    let mode = mouse.mode().unwrap_or(0xFF);
    let active = mouse.active_profile().unwrap_or(0);

    // ── Flash ──
    let (flash_ratio, flash_text) = match mouse.flash() {
        Ok(f) => {
            let usable = f.immediately_usable();
            let ratio = if f.total > 0 {
                1.0 - (usable as f32 / f.total as f32)
            } else {
                0.0
            };
            // 剩余百分比按「可立即使用的字节 / 总容量」算，和板载页数的口径一致。
            let free_pct = if f.total > 0 {
                (usable as f64 / f.total as f64 * 100.0).round() as u32
            } else {
                0
            };
            (
                ratio.clamp(0.0, 1.0),
                format!(
                    "剩 {free_pct}% · {} B 可用 / 共 {} B（{} 页）",
                    usable,
                    f.total,
                    usable / f.page_size().max(1)
                ),
            )
        }
        Err(_) => (0.0, "—".into()),
    };

    // ── 槽位 ──
    let profile_ids = mouse.profile_list().unwrap_or_default();
    let mut slots = Vec::new();
    for id in &profile_ids {
        let led = mouse.lighting(*id, cmd::region::STRIP).ok();
        let summary = match &led {
            Some(l) => light_detail_zh(l),
            None => "—".into(),
        };
        let (r, g, b) = slot_rgb(*id);
        slots.push(SlotRow {
            index: *id as i32,
            name: slot_label_zh(*id).into(),
            summary: summary.into(),
            active: *id == active,
            colour: Color::from_rgb_u8(r, g, b),
        });
    }
    // 临时工作层（槽 0）不在 profile_list 里，需要单独补一行。
    if active == 0 {
        let (r, g, b) = slot_rgb(0);
        slots.insert(
            0,
            SlotRow {
                index: 0,
                name: slot_label_zh(0).into(),
                summary: "不占 Flash，改动立即生效，断电即失".into(),
                active: true,
                colour: Color::from_rgb_u8(r, g, b),
            },
        );
    }

    let slot_name = format!("{} 号槽 · {}", active, slot_label_zh(active));

    // ── 按键绑定 ──
    let mut buttons = Vec::new();
    for (id, _name) in btn::ALL {
        // 滚轮上下滚不是可重绑定的按键，跳过，免得给用户一个改不了的条目。
        if *id == btn::WHEEL_UP || *id == btn::WHEEL_DOWN {
            continue;
        }
        match mouse.binding(active, *id, cmd::layer::NORMAL) {
            Ok(b) => {
                let (kind, arg) = binding_to_edit(&b);
                buttons.push(ButtonRow {
                    id: *id as i32,
                    name: button_label_zh(*id).into(),
                    action: describe_binding_zh(&b).into(),
                    supported: true,
                    kind,
                    arg: arg.into(),
                })
            }
            Err(_) => buttons.push(ButtonRow {
                id: *id as i32,
                name: button_label_zh(*id).into(),
                action: "（该槽未使用此按键）".into(),
                supported: false,
                kind: 8,
                arg: "".into(),
            }),
        }
    }

    // ── 宏 ──
    // 设备不给"列出全部宏"，只查登记表里记过的 ID，顺手把已经不在的清掉。
    let macro_ids = razer_ipc::known::retain_existing(|id| {
        mouse.macro_size(id).map(|s| s > 0).unwrap_or(false)
    });
    let mut macros = Vec::new();
    let mut total_bytes = 0u64;
    for id in &macro_ids {
        let size = mouse.macro_size(*id).unwrap_or(0);
        total_bytes += size as u64;
        let summary = match mouse.macro_read(*id) {
            Ok(bytes) => summarize_macro(&bytes),
            Err(_) => "（读取失败）".into(),
        };
        macros.push(MacroRow {
            id: *id as i32,
            id_hex: format!("0x{id:04X}").into(),
            size: format!("{size} B").into(),
            summary: summary.into(),
        });
    }

    // ── 灯效 ──
    let mut lights = Vec::new();
    let mut strip_effect = "—".to_string();
    let mut strip_bright = 0i32;
    let mut strip_rgb = (0x44, 0xd6, 0x2c);
    for (region, label) in [
        (cmd::region::WHEEL, "滚轮"),
        (cmd::region::LOGO, "Logo"),
        (cmd::region::STRIP, "底边灯带"),
    ] {
        let led = mouse.lighting(active, region).ok();
        let raw_bright = mouse.brightness(active, region).unwrap_or(0);
        let pct = (raw_bright as u16 * 100 / 255) as i32;
        let colour = led.as_ref().map(rgb_of).unwrap_or((0, 0, 0));
        let effect = led
            .as_ref()
            .map(light_detail_zh)
            .unwrap_or_else(|| "—".into());
        if region == cmd::region::STRIP {
            // 概览页只给一句短的，详情留给灯光页。
            strip_effect = match &led {
                Some(l) => light_name_zh(l),
                None => "—".into(),
            };
            strip_bright = pct;
            strip_rgb = colour;
        }
        lights.push(LightRow {
            region: label.into(),
            region_id: region as i32,
            effect: effect.into(),
            brightness: format!("{pct}%").into(),
            colour: Color::from_rgb_u8(colour.0, colour.1, colour.2),
        });
    }

    // ── DPI 与回报率 ──
    let dpi_text = match mouse.dpi(active) {
        Ok((x, y)) if x == y => format!("{x}"),
        Ok((x, y)) => format!("{x} × {y}"),
        Err(_) => "—".into(),
    };
    let polling_text = match mouse.polling_hz(active) {
        Ok(0) => "—".to_string(),
        Ok(hz) => format!("{hz} Hz"),
        Err(_) => "—".into(),
    };
    let stages = match mouse.dpi_stages(active) {
        Ok(s) => s
            .stages
            .iter()
            .enumerate()
            .map(|(i, (idx, x, y))| StageRow {
                index: i as i32,
                dpi: if x == y {
                    format!("{x} DPI")
                } else {
                    format!("{x} × {y} DPI")
                }
                .into(),
                active: *idx == s.active,
            })
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };

    Payload {
        path: path_display,
        firmware,
        mode_text: mode_text_zh(mode),
        connected: true,
        status: "已连接".to_string(),
        active,
        slot_name,
        flash_ratio,
        flash_text,
        buttons,
        macros,
        macro_count: macro_ids.len() as i32,
        macro_bytes: format!("{total_bytes} B"),
        lights,
        light_effect: strip_effect,
        light_brightness: strip_bright,
        light_rgb: strip_rgb,
        light_region: format!("底边灯带 (0x{:02X})", cmd::region::STRIP),
        strip_bright,
        dpi_text,
        polling_text,
        stages,
        slots,
    }
}

/// 把宏的原始字节压缩成一句概述。宏指令格式见 docs/PROTOCOL.md。
fn summarize_macro(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "（空）".into();
    }
    // decode 输出的就是编辑器里用的那种动作串（例如 "ctrl+c wait:200"），
    // 比十六进制字节有用得多。只在超长时截断，行尾标省略号。
    let text = macro_codec::decode(bytes);
    let mut short: String = text.chars().take(56).collect();
    if text.chars().count() > 56 {
        short.push_str(" …");
    }
    format!("{} B · {}", bytes.len(), short)
}

/// 首次采集完成后才把设备颜色同步到取色板；之后用户选了什么就保持什么，
/// 不再被每 3 秒一次的刷新覆盖。
static FIRST_LOAD: AtomicBool = AtomicBool::new(true);

// ── 设置页：配置备份 / 开机启动 ──
//
// 开机启动只写 HKCU 的 Run 项，不需要管理员权限。直接调 Win32 注册表 API，
// 不起子进程、不闪控制台，也不依赖任何外部命令。

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "RazerMate";

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn autostart_query() -> bool {
    unsafe {
        let sub = to_wide(RUN_SUBKEY);
        let name = to_wide(RUN_VALUE);
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return false;
        }
        let mut kind = 0u32;
        let mut size = 0u32;
        // lpdata 给 NULL 时只取类型和长度，够判断"这一项在不在"了。
        let found = RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        ) == 0;
        RegCloseKey(key);
        found
    }
}

fn autostart_set(on: bool) -> Result<(), String> {
    let agent = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("razermate-agent.exe")))
        .ok_or_else(|| "找不到 razermate-agent.exe".to_string())?;
    unsafe {
        let sub = to_wide(RUN_SUBKEY);
        let name = to_wide(RUN_VALUE);
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_READ | KEY_WRITE,
            &mut key,
        ) != 0
        {
            return Err("无法打开 HKCU 启动项".into());
        }
        let status = if on {
            // 引号是必须的：路径里可能有空格。
            let value = to_wide(&format!("\"{}\"", agent.display()));
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast::<u8>(),
                (value.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(key, name.as_ptr())
        };
        RegCloseKey(key);
        if status == 0 {
            Ok(())
        } else {
            Err(format!("注册表操作失败（代码 {status}）"))
        }
    }
}

fn backup_latest() -> std::path::PathBuf {
    paths::backup_dir().join("latest.txt")
}

/// 查注册表要起一个短命进程，不能塞进每 3 秒一次的 `apply()`。
/// 只在启动和点击按钮之后单独刷新。
fn settings_info() -> (String, bool, bool) {
    let info = match std::fs::read_to_string(backup_latest()) {
        Ok(t) => format!("latest.txt · {} 行", t.lines().count()),
        Err(_) => "还没有备份".into(),
    };
    (info, backup_latest().is_file(), autostart_query())
}

fn spawn_settings_refresh(weak: slint::Weak<AppWindow>, delay_ms: u64) {
    std::thread::spawn(move || {
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        let (info, exists, autostart) = settings_info();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_backup_info(SharedString::from(info.as_str()));
                ui.set_backup_exists(exists);
                ui.set_autostart_on(autostart);
            }
        });
    });
}

fn apply(ui: &AppWindow, p: Payload) {
    ui.set_connected(p.connected);
    ui.set_busy(false);
    ui.set_status_text(SharedString::from(p.status.as_str()));
    ui.set_dev_path(SharedString::from(p.path.as_str()));
    ui.set_firmware(SharedString::from(p.firmware.as_str()));
    ui.set_mode_text(SharedString::from(p.mode_text.as_str()));

    ui.set_active_slot(p.active as i32);
    ui.set_slot_name(SharedString::from(p.slot_name.as_str()));
    ui.set_flash_ratio(p.flash_ratio as f32);
    ui.set_flash_text(SharedString::from(p.flash_text.as_str()));

    ui.set_button_count(p.buttons.len() as i32);
    // 第一次拿到按键列表时默认选中左键，免得右侧编辑区一片空白。
    // 只自动选一次：用户按「取消」收起编辑区后，不该被下一次刷新又弹开。
    // 不再自动选中左键：空状态要留给「全部按键 + 当前动作」清单，
    // 这样打开页面就能看全，不用先点一下图。
    ui.set_buttons(ModelRc::new(VecModel::from(p.buttons)));
    // 选中的按键动作文案：列表刷新后跟着更新（写入成功后这里显示的就是新值）。
    let sel = ui.get_sel_button();
    if sel >= 0 {
        let action = ui
            .get_buttons()
            .iter()
            .find(|b| b.id == sel)
            .map(|b| b.action.clone())
            .unwrap_or_else(|| "（该槽未使用此按键）".into());
        ui.set_sel_button_action(action);
    }

    ui.set_macro_count(p.macro_count);
    ui.set_macro_bytes(SharedString::from(p.macro_bytes.as_str()));
    ui.set_macros(ModelRc::new(VecModel::from(p.macros)));

    ui.set_lights(ModelRc::new(VecModel::from(p.lights)));
    ui.set_light_effect(SharedString::from(p.light_effect.as_str()));
    ui.set_light_brightness(p.light_brightness);
    ui.set_light_colour(Color::from_rgb_u8(
        p.light_rgb.0,
        p.light_rgb.1,
        p.light_rgb.2,
    ));
    ui.set_light_region(SharedString::from(p.light_region.as_str()));

    ui.set_dpi_text(SharedString::from(p.dpi_text.as_str()));
    ui.set_polling_text(SharedString::from(p.polling_text.as_str()));
    ui.set_stages(ModelRc::new(VecModel::from(p.stages)));

    ui.set_slots(ModelRc::new(VecModel::from(p.slots)));

    // 只在首次或用户没动过时同步编辑控件，避免每 3 秒刷新把用户正在拖的滑条拽回去。
    if ui.get_sel_bright() == 100 && p.strip_bright != 100 {
        ui.set_sel_bright(p.strip_bright);
    }
    if FIRST_LOAD.swap(false, Ordering::Relaxed) && p.light_rgb != (0, 0, 0) {
        ui.set_edit_colour(Color::from_rgb_u8(
            p.light_rgb.0,
            p.light_rgb.1,
            p.light_rgb.2,
        ));
    }
    if ui.get_dpi_input().is_empty() && p.dpi_text != "—" {
        // DPI 可能是 "2700" 或 "2700 × 2700"，编辑框只放 X 值。
        let first = p.dpi_text.split('×').next().unwrap_or("").trim().to_string();
        ui.set_dpi_input(SharedString::from(first));
    }
}

fn spawn_refresh(weak: slint::Weak<AppWindow>) {
    if let Some(ui) = weak.upgrade() {
        ui.set_busy(true);
    }
    std::thread::spawn(move || {
        let payload = collect();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                apply(&ui, payload);
            }
        });
    });
}

/// 执行一次写操作：开设备 → 跑闭包 → 重新采集 → 回主线程刷新 + 弹提示。
fn spawn_op<F>(weak: slint::Weak<AppWindow>, op: F)
where
    F: FnOnce(&Mouse) -> Result<String, String> + Send + 'static,
{
    if let Some(ui) = weak.upgrade() {
        ui.set_busy(true);
    }
    std::thread::spawn(move || {
        let outcome = match Mouse::open() {
            Ok(m) => op(&m),
            Err(e) => Err(format!("无法打开设备：{e}")),
        };
        let payload = collect();
        let toast = match outcome {
            Ok(msg) => msg,
            Err(err) => format!("失败 · {err}"),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                apply(&ui, payload);
                show_toast(ui.as_weak(), toast);
            }
        });
    });
}

fn show_toast(weak: slint::Weak<AppWindow>, text: String) {
    let Some(ui) = weak.upgrade() else { return };
    ui.set_toast(SharedString::from(text.as_str()));
    // 3 秒后自动消失；只清掉自己这条，别把后到的新提示吞了。
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                if ui.get_toast() == SharedString::from(text.as_str()) {
                    ui.set_toast(SharedString::from(""));
                }
            }
        });
    });
}

/// 把界面上的 0–100 速度映射到协议里的 0–255。
fn speed_percent_to_raw(pct: i32) -> u8 {
    ((pct.clamp(0, 100) as u32 * 255) / 100) as u8
}

// ───────────────────── 后台服务与自动切换 ─────────────────────
//
// GUI 与 agent 之间只走两条路：
//   * 运行状态、暂停、退出、当前前台程序 —— 走命名管道，因为只有 agent 知道；
//   * 规则本身 —— 直接读写 rules.conf，再让 agent RELOAD。
//     规则是纯文本、格式稳定，GUI 自己改自己读最简单，也让用户能手改同一个文件。

struct AgentView {
    running: bool,
    paused: bool,
    detail: String,
    fg: String,
    rules: Vec<RuleRow>,
    enabled: bool,
    poll: i32,
    default_slot: i32,
}

fn to_rule_rows(r: &Rules) -> Vec<RuleRow> {
    r.rules
        .iter()
        .map(|rule| RuleRow {
            exe: rule.exe.clone().into(),
            slot: rule.slot as i32,
            slot_name: format!("槽 {} · {}", rule.slot, slot_label_zh(rule.slot)).into(),
        })
        .collect()
}

fn read_agent() -> AgentView {
    let rules = Rules::load_from(&paths::rules_path());

    match client::call("STATUS") {
        Ok(reply) => {
            let paused = client::field(&reply, "paused").as_deref() == Some("1");
            let connected = client::field(&reply, "connected").as_deref() == Some("1");
            let slot = client::field(&reply, "slot")
                .and_then(|s| s.parse::<u8>().ok())
                .unwrap_or(0);
            let fg = client::call("FG")
                .ok()
                .and_then(|r| client::field(&r, "exe"))
                .filter(|s| s != "-")
                .unwrap_or_else(|| "—".into());

            let detail = if connected {
                format!(
                    "运行中 · 当前 {slot} 号槽 · {} 条规则",
                    rules.rules.len()
                )
            } else {
                "运行中，但鼠标不在线".to_string()
            };

            AgentView {
                running: true,
                paused,
                detail,
                fg,
                rules: to_rule_rows(&rules),
                enabled: rules.enabled,
                poll: rules.poll_ms as i32,
                default_slot: rules.default_slot as i32,
            }
        }
        Err(_) => AgentView {
            detail: "未运行".to_string(),
            running: false,
            paused: false,
            fg: "—".into(),
            rules: to_rule_rows(&rules),
            enabled: rules.enabled,
            poll: rules.poll_ms as i32,
            default_slot: rules.default_slot as i32,
        },
    }
}

fn ui_paused(weak: &slint::Weak<AppWindow>) -> bool {
    weak.upgrade().map(|ui| ui.get_agent_paused()).unwrap_or(false)
}

// ── 在线模式 / 虚拟槽 ──────────────────────────────────────────────────────

/// 把索引转成界面列表行。current 来自 agent 的 VSSTATUS。
fn to_vs_rows(idx: &VsIndex, current: &str) -> Vec<VsRow> {
    idx.slots.iter().map(|s| VsRow {
        id: s.id.clone().into(),
        name: s.name.clone().into(),
        exes: s.exes.join(", ").into(),
        current: s.id == current,
    }).collect()
}

/// 从 agent 读虚拟槽状态（启用 / 当前生效 / 列表），填进界面。
fn refresh_vslots(ui: &AppWindow) {
    let idx = VsIndex::load();
    let current = client::call("VSSTATUS").ok()
        .and_then(|r| client::field(&r, "current"))
        .filter(|s| s != "-")
        .unwrap_or_default();
    // 以本地配置为权威：agent 未启动时 VSSTATUS 读不到，不能因此误显示离线。
    // agent 启动后会通过 VSRELOAD 同步成同一份配置。
    let enabled = idx.enabled;
    ui.set_vs_enabled(enabled);
    ui.set_vs_current(SharedString::from(current.as_str()));
    ui.set_vs_slots(ModelRc::new(VecModel::from(to_vs_rows(&idx, &current))));
}

/// 把某个虚拟槽的完整配置装进编辑区；不存在则清空。
fn load_vs_into_editor(ui: &AppWindow, id: &str) {
    ui.set_vs_edit_id(SharedString::from(id));
    let Some(cfg) = SlotConfig::load(id) else {
        ui.set_vs_edit_name("".into());
        ui.set_vs_edit_exes("".into());
        ui.set_vs_edit_dpi("".into());
        return;
    };
    ui.set_vs_edit_name(SharedString::from(cfg.name.as_str()));
    let exes = VsIndex::load().find(id).map(|m| m.exes.join(", ")).unwrap_or_default();
    ui.set_vs_edit_exes(SharedString::from(exes.as_str()));
    ui.set_vs_edit_dpi(SharedString::from(
        cfg.dpi.map(|(x, y)| if x == y { x.to_string() } else { format!("{x},{y}") }).unwrap_or_default().as_str()
    ));
    ui.set_vs_edit_polling(cfg.polling.unwrap_or(1000) as i32);
    // 三区灯效逐个读回：效果、颜色、亮度。
    for (region, effect_prop) in [
        (cmd::region::STRIP, 0),
        (cmd::region::LOGO, 1),
        (cmd::region::WHEEL, 2),
    ] {
        if let Some(led) = cfg.leds.iter().find(|l| l.region == region) {
            let kind = effect_kind(&led.effect);
            let colour = led.rgb.map(|(r, g, b)| Color::from_rgb_u8(r, g, b)).unwrap_or_else(|| Color::from_rgb_u8(68, 214, 44));
            let raw = cfg.brightness.iter().find(|(r, _)| *r == region).map(|(_, v)| ((*v as i32) * 100 / 255).min(100)).unwrap_or(60);
            match effect_prop {
                0 => { ui.set_vs_edit_strip_effect(kind); ui.set_vs_edit_strip_colour(colour); ui.set_vs_edit_strip_bright(raw); }
                1 => { ui.set_vs_edit_logo_effect(kind); ui.set_vs_edit_logo_colour(colour); ui.set_vs_edit_logo_bright(raw); }
                _ => { ui.set_vs_edit_wheel_effect(kind); ui.set_vs_edit_wheel_colour(colour); ui.set_vs_edit_wheel_bright(raw); }
            }
        }
    }
}

fn effect_kind(name: &str) -> i32 {
    match name {
        "spectrum" => 3,
        "wave" => 4,
        _ => 1,
    }
}

fn effect_name(kind: i32) -> &'static str {
    match kind {
        3 => "spectrum",
        4 => "wave",
        _ => "static",
    }
}

/// 从界面编辑区拼出一份完整的虚拟槽配置并存盘。
fn save_vs_from_editor(ui: &AppWindow) -> Result<String, String> {
    let id = ui.get_vs_edit_id().to_string();
    if id.is_empty() {
        return Err("请先新建或选择一个虚拟槽".into());
    }
    let name = ui.get_vs_edit_name().to_string();
    let exes: Vec<String> = ui.get_vs_edit_exes().to_string()
        .split([',', ' ', ';']).map(str::trim).filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase()).collect();

    // 索引：更新这个槽的名字与 exe 规则。
    let mut idx = VsIndex::load();
    idx.upsert(SlotMeta { id: id.clone(), name: name.clone(), exes });
    idx.save().map_err(|e| format!("保存索引失败：{e}"))?;

    // 完整配置：三区灯效 + 亮度 + DPI + 回报率。
    let mut cfg = SlotConfig::load(&id).unwrap_or_default();
    cfg.name = name;
    let regions = [cmd::region::STRIP, cmd::region::LOGO, cmd::region::WHEEL];
    cfg.leds.retain(|l| !regions.contains(&l.region));
    cfg.brightness.retain(|(r, _)| !regions.contains(r));
    for (idx, region) in regions.iter().enumerate() {
        let (kind, colour, bright) = match idx {
            0 => (ui.get_vs_edit_strip_effect(), ui.get_vs_edit_strip_colour(), ui.get_vs_edit_strip_bright()),
            1 => (ui.get_vs_edit_logo_effect(), ui.get_vs_edit_logo_colour(), ui.get_vs_edit_logo_bright()),
            _ => (ui.get_vs_edit_wheel_effect(), ui.get_vs_edit_wheel_colour(), ui.get_vs_edit_wheel_bright()),
        };
        cfg.leds.push(LedSetting {
            region: *region,
            effect: effect_name(kind).to_string(),
            rgb: Some((colour.red(), colour.green(), colour.blue())),
            mode: 1,
            speed: 5,
        });
        let raw = ((bright as u16) * 255 / 100).min(255) as u8;
        cfg.brightness.push((*region, raw));
    }

    let dpi_text = ui.get_vs_edit_dpi().to_string();
    cfg.dpi = dpi_text.trim().parse::<u16>().ok().map(|v| (v, v));
    let polling = ui.get_vs_edit_polling() as u16;
    cfg.polling = if polling == 125 || polling == 500 || polling == 1000 { Some(polling) } else { None };
    cfg.save(&id).map_err(|e| format!("保存配置失败：{e}"))?;

    let _ = client::call("VSRELOAD");
    Ok(format!("虚拟槽「{}」已保存", cfg.name))
}

fn apply_agent(ui: &AppWindow, v: AgentView) {
    ui.set_agent_running(v.running);
    ui.set_agent_paused(v.paused);
    ui.set_agent_detail(SharedString::from(v.detail.as_str()));
    ui.set_fg_exe(SharedString::from(v.fg.as_str()));
    ui.set_rules(ModelRc::new(VecModel::from(v.rules)));
    ui.set_auto_enabled(v.enabled);
    ui.set_auto_poll(v.poll);
    ui.set_auto_default(v.default_slot);
    refresh_vslots(ui);
}

fn spawn_agent_refresh(weak: slint::Weak<AppWindow>) {
    std::thread::spawn(move || {
        let view = read_agent();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                apply_agent(&ui, view);
            }
        });
    });
}

/// 隔一会儿再查一次。改完规则后 agent 要等下一拍才真正切槽，
/// 立刻查会读到旧槽位，界面上那个「当前 N 号槽」就会显得对不上。
fn spawn_agent_refresh_later(weak: slint::Weak<AppWindow>, delay_ms: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay_ms));
        let view = read_agent();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                apply_agent(&ui, view);
            }
        });
    });
}

/// 与 razermate.exe 同目录的 razermate-agent.exe。
fn agent_exe_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("razermate-agent.exe")))
}

fn start_agent() -> Result<String, String> {
    let exe = agent_exe_path().ok_or("取不到程序目录")?;
    if !exe.exists() {
        return Err(format!("找不到 {exe:?}，请把 razermate-agent.exe 放在同一目录"));
    }
    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| format!("启动失败：{e}"))?;
    Ok("后台服务已启动".into())
}

fn save_rules_and_reload(r: &Rules) -> Result<String, String> {
    r.save_to(&paths::rules_path())
        .map_err(|e| format!("写入规则文件失败：{e}"))?;
    // agent 没在跑也不算错：下次启动时会读到这份文件。
    let _ = client::call("RELOAD");
    Ok(format!("已保存 {} 条规则", r.rules.len()))
}

// ───────────────────── 宏编辑 ─────────────────────
//
// 编辑状态放在进程里，不放在 Slint 模型里：模型只负责显示（每行一句人话），
// 真正的步骤列表是 Vec<Step>，因为「往上挪一格」这种操作要动顺序，用显示字符串
// 反解析回去既慢又容易丢信息。

static MACRO_EDIT: Mutex<Option<(u16, Vec<Step>)>> = Mutex::new(None);

fn step_kind(s: &Step) -> i32 {
    match s {
        Step::Combo(_) => 0,
        Step::Text(_) => 1,
        Step::Wait(_) => 2,
        Step::Click(_) => 3,
        Step::Wheel(_) => 4,
        Step::Media(_) => 5,
    }
}

fn kind_label_zh(kind: i32) -> &'static str {
    match kind {
        0 => "按键",
        1 => "文本",
        2 => "等待",
        3 => "鼠标",
        4 => "滚轮",
        5 => "媒体键",
        _ => "?",
    }
}

/// 把一步渲染成给人看的一句话。参数本身留在 `step_arg_of` 里给编辑框用。
fn step_display(s: &Step) -> String {
    match s {
        Step::Combo(c) => format!("按下 {c}"),
        Step::Text(t) => format!("输入「{t}」"),
        Step::Wait(ms) => format!("等待 {ms} ms"),
        Step::Click(b) => format!(
            "点击{}键",
            match b {
                MouseButton::Left => "左",
                MouseButton::Right => "右",
                MouseButton::Middle => "中",
            }
        ),
        Step::Wheel(d) => format!("滚轮 {d:+} 格"),
        Step::Media(m) => format!("媒体键 {m}"),
    }
}

fn step_arg_of(s: &Step) -> String {
    match s {
        Step::Combo(c) => c.clone(),
        Step::Text(t) => t.clone(),
        Step::Wait(ms) => ms.to_string(),
        Step::Click(b) => match b {
            MouseButton::Left => "left",
            MouseButton::Right => "right",
            MouseButton::Middle => "middle",
        }
        .to_string(),
        Step::Wheel(d) => d.to_string(),
        Step::Media(m) => m.clone(),
    }
}

fn make_step(kind: i32, arg: &str) -> Result<Step, String> {
    let a = arg.trim();
    if a.is_empty() {
        return Err("参数不能为空".into());
    }
    Ok(match kind {
        0 => Step::Combo(a.to_string()),
        1 => Step::Text(a.to_string()),
        2 => Step::Wait(
            a.parse::<u32>()
                .map_err(|_| "等待要填毫秒数，例如 120")?,
        ),
        3 => Step::Click(
            MouseButton::parse(a).ok_or("鼠标键只支持 left / right / middle")?,
        ),
        4 => Step::Wheel(a.parse::<i8>().map_err(|_| "滚轮填 ±格数，例如 1 或 -1")?),
        5 => Step::Media(a.to_ascii_lowercase()),
        _ => return Err("未知步骤类型".into()),
    })
}

fn publish_macro(ui: &AppWindow) {
    let guard = MACRO_EDIT.lock().unwrap();
    let Some((id, steps)) = guard.as_ref() else {
        ui.set_macro_edit_id(-1);
        ui.set_macro_steps(ModelRc::new(VecModel::from(Vec::<StepRow>::new())));
        ui.set_macro_bytes_preview(SharedString::from("—"));
        return;
    };

    let rows: Vec<StepRow> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let kind = step_kind(s);
            StepRow {
                index: i as i32,
                kind,
                kind_label: kind_label_zh(kind).into(),
                text: step_display(s).into(),
                bytes: macro_codec::encode_step(s)
                    .map(|b| b.len() as i32)
                    .unwrap_or(0),
            }
        })
        .collect();
    let total = macro_codec::encode_steps(steps).map(|b| b.len()).unwrap_or(0);

    ui.set_macro_edit_id(*id as i32);
    ui.set_macro_edit_name(SharedString::from(format!("0x{id:04X}")));
    ui.set_macro_steps(ModelRc::new(VecModel::from(rows)));
    ui.set_macro_bytes_preview(SharedString::from(format!(
        "{} 步 · {} B",
        steps.len(),
        total
    )));
}

/// 改一下当前打开的宏，然后刷新显示。`f` 返回 Err 时不改动任何东西。
fn edit_macro<F>(ui: &AppWindow, f: F) -> Result<(), String>
where
    F: FnOnce(&mut Vec<Step>) -> Result<(), String>,
{
    {
        let mut guard = MACRO_EDIT.lock().unwrap();
        let Some((_, steps)) = guard.as_mut() else {
            return Err("没有打开任何宏".into());
        };
        f(steps)?;
    }
    publish_macro(ui);
    Ok(())
}

// ───────────────────── 窗口图标 ─────────────────────
//
// Slint 的 winit 后端不会去读 exe 里的图标资源，窗口默认挂系统通用图标，
// 任务栏和 Alt+Tab 里看到的就是那个白色小窗口。窗口建好后用 WM_SETICON
// 把编译进 exe 的雷蛇图标（资源 ID 1，见 assets/app.rc）换上去。

/// 按自己的 PID 找顶层窗口，换上 exe 里的图标。返回是否设置成功。
///
/// 图标用 ExtractIconExW 直接从 exe 文件里取：它按文件解析 PE 资源，不依赖
/// 模块句柄的 FindResource（本程序里那条路会报 ERROR_RESOURCE_TYPE_NOT_FOUND），
/// 拿到 HICON 后 WM_SETICON 给窗口的大小图标各挂一个。
#[cfg(windows)]
fn install_window_icon() -> bool {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::UI::Shell::ExtractIconExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, SendMessageW,
        ICON_BIG, ICON_SMALL, WM_SETICON,
    };

    static FOUND: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn pick(hwnd: HWND, _: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 || GetWindowTextLengthW(hwnd) == 0 {
            return 1;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == std::process::id() {
            FOUND.store(hwnd as isize, Ordering::SeqCst);
            return 0; // 找到就停
        }
        1
    }

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let exe_w: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();

    unsafe {
        EnumWindows(Some(pick), 0);
        let hwnd = FOUND.load(Ordering::SeqCst) as HWND;
        if hwnd.is_null() {
            return false;
        }

        let mut big: *mut c_void = std::ptr::null_mut();
        let mut small: *mut c_void = std::ptr::null_mut();
        if ExtractIconExW(exe_w.as_ptr(), 0, &mut big, &mut small, 1) == 0 {
            return false;
        }
        if !big.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
        }
        if !small.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        }
        !big.is_null() || !small.is_null()
    }
}

/// 窗口是 `ui.run()` 里才创建的，所以延迟几拍去装；装不上就再等一拍。
#[cfg(windows)]
fn arm_window_icon(attempt: u32) {
    slint::Timer::single_shot(std::time::Duration::from_millis(80), move || {
        if install_window_icon() || attempt >= 10 {
            return;
        }
        arm_window_icon(attempt + 1);
    });
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // `--page N` 直接打开第 N 页（0=概览 1=自定义 2=宏 3=自动切换）。
    // 开发期用来逐页截图核对渲染，也方便脚本化冒烟。
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--page") {
        if let Some(n) = args.get(i + 1).and_then(|s| s.parse::<i32>().ok()) {
            ui.set_page(n.clamp(0, 3));
        }
    }

    // ── 刷新 ──
    let weak = ui.as_weak();
    ui.on_refresh(move || spawn_refresh(weak.clone()));

    let weak = ui.as_weak();
    ui.on_quit_app(move || {
        if let Some(ui) = weak.upgrade() {
            let _ = ui.hide();
        }
    });

    // ── 选择按键：把当前绑定装进编辑区 ──
    let weak = ui.as_weak();
    ui.on_pick_button(move |id| {
        if let Some(ui) = weak.upgrade() {
            let model = ui.get_buttons();
            for i in 0..model.row_count() {
                if let Some(row) = model.row_data(i) {
                    if row.id == id {
                        ui.set_sel_button(id);
                        ui.set_sel_button_name(row.name.clone());
                        ui.set_sel_button_action(row.action.clone());
                        ui.set_edit_kind(row.kind);
                        ui.set_edit_arg(row.arg.clone());
                        // 宏类读回形如 0x0101x3 / 0x0101，取十六进制前缀作为点选基线。
                        let base = if matches!(row.kind, 4 | 9 | 10) {
                            let a = row.arg.as_str();
                            let end = a.find('x').unwrap_or(a.len());
                            a[..end].to_string()
                        } else {
                            String::new()
                        };
                        ui.set_macro_base(base.into());
                        ui.set_recording(false);
                        ui.set_advanced_arg(false);
                        return;
                    }
                }
            }
            // 该槽没上报这个按键：仍然切到它，只是没有可改的动作，
            // 免得用户点了图上某个键却毫无反应。
            ui.set_sel_button(id);
            ui.set_sel_button_name(button_label_zh(id as u8).into());
            ui.set_sel_button_action("（该槽未使用此按键）".into());
            ui.set_edit_kind(8);
            ui.set_edit_arg(SharedString::new());
            ui.set_recording(false);
            ui.set_advanced_arg(false);
        }
    });

    let weak = ui.as_weak();
    ui.on_cancel_binding(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_sel_button(-1);
            ui.set_recording(false);
            ui.set_advanced_arg(false);
        }
    });

    // 录入组合键：Slint 的 KeyEvent.text + 修饰键 → 设备可用的 key:... 参数。
    let weak = ui.as_weak();
    ui.on_key_captured(move |text, ctrl, shift, alt, win| {
        let Some(ui) = weak.upgrade() else { return };
        let key = key_name_from_slint_text(&text);
        if key.is_none() {
            ui.set_record_hint("请按下主键（Ctrl / Shift / Alt / Win 只是修饰键）…".into());
            return;
        }
        let arg = chord_token(&key.unwrap(), ctrl, shift, alt, win);
        // 校验一次：设备要认得这个组合才算录成功。
        if parse_action(&format!("key:{arg}")).is_none() {
            ui.set_record_hint("这个键设备不支持，请换一个再按…".into());
            return;
        }
        ui.set_edit_kind(2);
        ui.set_edit_arg(arg.clone().into());
        ui.set_record_hint(format!("已录入 {arg}").into());
        ui.set_recording(false);
        ui.set_advanced_arg(false);
    });

    let weak = ui.as_weak();
    ui.on_finish_recording(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_recording(false);
        }
    });

    // ── 写按键绑定 ──
    let weak = ui.as_weak();
    ui.on_apply_binding(move |button, kind, arg| {
        let spec = match edit_to_spec(kind, arg.as_str()) {
            Some(s) => s,
            None => {
                show_toast(weak.clone(), "该类型不支持写入".into());
                return;
            }
        };
        let binding = match parse_action(&spec) {
            Some(b) => b,
            None => {
                show_toast(
                    weak.clone(),
                    format!("参数无法解析：{spec}（示例：ctrl+w / 0x0101x1 / play）"),
                );
                return;
            }
        };
        spawn_op(weak.clone(), move |m| {
            let profile = m.active_profile().map_err(|e| e.to_string())?;
            let got = m
                .set_binding(profile, button as u8, cmd::layer::NORMAL, binding)
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "已写入 {} 号槽：{} → {}",
                profile,
                button_label_zh(button as u8),
                describe_binding_zh(&got)
            ))
        });
    });

    // ── 选灯区：把该区的亮度与颜色同步进控件，避免一进来就是默认值 ──
    let weak = ui.as_weak();
    ui.on_pick_region(move |region| {
        if let Some(ui) = weak.upgrade() {
            ui.set_sel_region(region);
            let model = ui.get_lights();
            for i in 0..model.row_count() {
                if let Some(row) = model.row_data(i) {
                    if row.region_id == region {
                        let txt = row.brightness.to_string();
                        let pct = txt.trim_end_matches('%').parse::<i32>().unwrap_or(100);
                        ui.set_sel_bright(pct);
                        ui.set_edit_colour(row.colour);
                        break;
                    }
                }
            }
        }
    });

    // ── 写灯效 ──
    let weak = ui.as_weak();
    ui.on_apply_effect(move |region, effect, colour, speed| {
        let rgb = (
            colour.red(),
            colour.green(),
            colour.blue(),
        );
        spawn_op(weak.clone(), move |m| {
            let profile = m.active_profile().map_err(|e| e.to_string())?;
            let region = region as u8;
            let l = match effect {
                3 => m.set_spectrum(profile, region),
                4 => m.set_wave(profile, region, 1, speed_percent_to_raw(speed)),
                _ => m.set_static(profile, region, rgb),
            }
            .map_err(|e| e.to_string())?;
            Ok(format!(
                "已写入 {} 号槽 · {}：{}",
                profile,
                region_label(region),
                light_detail_zh(&l)
            ))
        });
    });

    // ── 写亮度 ──
    let weak = ui.as_weak();
    ui.on_apply_brightness(move |region, pct| {
        spawn_op(weak.clone(), move |m| {
            let profile = m.active_profile().map_err(|e| e.to_string())?;
            let got = m
                .set_brightness(profile, region as u8, pct.clamp(0, 100) as u8)
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "已写入 {} 号槽 · {} 亮度 {got}%",
                profile,
                region_label(region as u8)
            ))
        });
    });

    // ── 灯带预览 ──
    // 逐颗是运行时效果，不写 Flash。预览前把板载灯效存到文件，
    // 这样即使关掉程序、隔天再打开，点「还原」也能找回来。
    let weak = ui.as_weak();
    ui.on_preview_led(move |mode| {
        spawn_op(weak.clone(), move |m| {
            let profile = m.active_profile().map_err(|e| e.to_string())?;
            let region = cmd::region::STRIP;
            let n = cmd::region::STRIP_LEDS as usize;
            match mode {
                0 | 1 => {
                    let now = m.lighting(profile, region).map_err(|e| e.to_string())?;
                    // 只在板载状态下备份；连续预览时不能把 CUSTOM 帧记成板载效果。
                    if now.effect != cmd::effect::CUSTOM {
                        let rgb = now.colors.first().copied().unwrap_or((68, 214, 44));
                        let _ = razer_ipc::ledbak::save(now.effect, now.mode, now.speed, rgb);
                    }
                    let base = razer_ipc::ledbak::load()
                        .map(|(_, _, _, c)| c)
                        .or_else(|| now.colors.first().copied())
                        .unwrap_or((68, 214, 44));
                    let colours = if mode == 0 {
                        razer_core::led_rainbow(n, 0)
                    } else {
                        razer_core::led_gradient(n, base)
                    };
                    m.set_custom(profile, region, &colours)
                        .map_err(|e| e.to_string())?;
                    Ok(format!(
                        "已预览{} · {n} 颗灯，点「还原」恢复板载灯效",
                        if mode == 0 { "彩虹" } else { "渐变" }
                    ))
                }
                2 => {
                    let now = m.lighting(profile, region).map_err(|e| e.to_string())?;
                    let board = razer_ipc::ledbak::load();
                    if now.effect == cmd::effect::CUSTOM && board.is_none() {
                        return Err("当前停在逐灯预览且没有板载灯效备份，无法安全还原".into());
                    }
                    let target = board
                        .map(|(effect, mode, speed, rgb)| cmd::Lighting {
                            effect,
                            mode,
                            speed,
                            colors: vec![rgb],
                        })
                        .unwrap_or(now);
                    let (frames, secs) = m
                        .led_animate(profile, region, &target, 60)
                        .map_err(|e| e.to_string())?;
                    razer_ipc::ledbak::clear();
                    Ok(format!("流动 {frames} 帧 / {secs:.1} s，已自动还原"))
                }
                3 => {
                    let Some((effect, md, speed, rgb)) = razer_ipc::ledbak::load() else {
                        return Err("没有可还原的灯效备份（还没预览过，或已经还原过了）".into());
                    };
                    let old = cmd::Lighting {
                        effect,
                        mode: md,
                        speed,
                        colors: vec![rgb],
                    };
                    let back = m
                        .led_restore(profile, region, &old)
                        .map_err(|e| e.to_string())?;
                    razer_ipc::ledbak::clear();
                    Ok(format!(
                        "已还原：{}",
                        razer_core::light_detail_zh(&back)
                    ))
                }
                _ => Err("未知预览模式".into()),
            }
        });
    });

    // ── 写 DPI ──
    let weak = ui.as_weak();
    ui.on_apply_dpi(move |text| {
        let dpi = match text.trim().parse::<u16>() {
            Ok(v) if (100..=26000).contains(&v) => v,
            _ => {
                show_toast(weak.clone(), "DPI 需要 100–26000 之间的整数".into());
                return;
            }
        };
        spawn_op(weak.clone(), move |m| {
            let profile = m.active_profile().map_err(|e| e.to_string())?;
            let (x, y) = m.set_dpi(profile, dpi, dpi).map_err(|e| e.to_string())?;
            Ok(format!("已写入 {} 号槽 DPI：{x} × {y}", profile))
        });
    });

    // ── 写回报率 ──
    let weak = ui.as_weak();
    ui.on_apply_polling(move |hz| {
        spawn_op(weak.clone(), move |m| {
            let profile = m.active_profile().map_err(|e| e.to_string())?;
            let got = m
                .set_polling_hz(profile, hz as u16)
                .map_err(|e| e.to_string())?;
            Ok(format!("已写入 {profile} 号槽回报率：{got} Hz"))
        });
    });

    // ── 切槽 ──
    let weak = ui.as_weak();
    ui.on_use_slot(move |slot| {
        spawn_op(weak.clone(), move |m| {
            let got = m
                .set_active_profile(slot as u8)
                .map_err(|e| e.to_string())?;
            Ok(format!("已切换到 {} 号槽 · {}", got, slot_label_zh(got)))
        });
    });

    spawn_refresh(ui.as_weak());

    #[cfg(windows)]
    arm_window_icon(0);

    // ── 自动切换页 ──
    let weak = ui.as_weak();
    ui.on_refresh_agent(move || spawn_agent_refresh(weak.clone()));

    let weak = ui.as_weak();
    ui.on_agent_start(move || {
        match start_agent() {
            Ok(msg) => show_toast(weak.clone(), msg),
            Err(e) => show_toast(weak.clone(), format!("失败 · {e}")),
        }
        // agent 起来要几十毫秒才建好管道，稍等一下再查状态，否则必然读到「未运行」。
        let weak2 = weak.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let view = read_agent();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak2.upgrade() {
                    apply_agent(&ui, view);
                }
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_agent_stop(move || {
        match client::call("QUIT") {
            Ok(_) => show_toast(weak.clone(), "后台服务已退出".into()),
            Err(e) => show_toast(weak.clone(), format!("失败 · {e}")),
        }
        let weak2 = weak.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            let view = read_agent();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak2.upgrade() {
                    apply_agent(&ui, view);
                }
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_agent_toggle_pause(move || {
        let want = !ui_paused(&weak);
        let cmd = if want { "PAUSE 1" } else { "PAUSE 0" };
        match client::call(cmd) {
            Ok(_) => show_toast(
                weak.clone(),
                if want {
                    "已暂停自动切换".into()
                } else {
                    "已恢复自动切换".into()
                },
            ),
            Err(e) => show_toast(weak.clone(), format!("失败 · {e}")),
        }
        spawn_agent_refresh(weak.clone());
    });

    // ── 规则增删 ──
    let weak = ui.as_weak();
    ui.on_add_rule(move |exe, slot| {
        let exe = exe.trim().to_ascii_lowercase();
        if exe.is_empty() {
            show_toast(weak.clone(), "程序名不能为空".into());
            return;
        }
        if !(1..=5).contains(&slot) {
            show_toast(weak.clone(), "槽位需要在 1–5 之间".into());
            return;
        }
        let mut rules = Rules::load_from(&paths::rules_path());
        rules.set_rule(&exe, slot as u8);
        match save_rules_and_reload(&rules) {
            Ok(msg) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_new_exe(SharedString::from(""));
                }
                show_toast(weak.clone(), format!("{msg} · {exe} → 槽 {slot}"));
            }
            Err(e) => show_toast(weak.clone(), e),
        }
        spawn_agent_refresh(weak.clone());
        spawn_agent_refresh_later(weak.clone(), 1200);
    });

    let weak = ui.as_weak();
    ui.on_remove_rule(move |exe| {
        let mut rules = Rules::load_from(&paths::rules_path());
        rules.set_rule(exe.as_str(), 0);
        match save_rules_and_reload(&rules) {
            Ok(_) => show_toast(weak.clone(), format!("已删除 {exe}")),
            Err(e) => show_toast(weak.clone(), e),
        }
        spawn_agent_refresh(weak.clone());
        spawn_agent_refresh_later(weak.clone(), 1200);
    });

    // ── 全局选项 ──
    let weak = ui.as_weak();
    ui.on_apply_auto(move |enabled, poll, default_slot| {
        let mut rules = Rules::load_from(&paths::rules_path());
        rules.enabled = enabled;
        rules.poll_ms = poll.clamp(200, 10_000) as u32;
        rules.default_slot = default_slot.clamp(0, 5) as u8;
        match save_rules_and_reload(&rules) {
            Ok(msg) => show_toast(weak.clone(), msg),
            Err(e) => show_toast(weak.clone(), e),
        }
        spawn_agent_refresh(weak.clone());
        spawn_agent_refresh_later(weak.clone(), 1200);
    });

    // ── 在线模式 / 虚拟槽 ──
    let weak = ui.as_weak();
    ui.on_vs_refresh(move || {
        if let Some(ui) = weak.upgrade() { refresh_vslots(&ui); }
    });

    let weak = ui.as_weak();
    ui.on_vs_pick(move |id| {
        if let Some(ui) = weak.upgrade() {
            load_vs_into_editor(&ui, id.as_str());
        }
    });

    let weak = ui.as_weak();
    ui.on_vs_new(move || {
        if let Some(ui) = weak.upgrade() {
            // 生成一个未占用的 ID：v1、v2……
            let idx = VsIndex::load();
            let mut n = 1;
            while idx.find(&format!("v{n}")).is_some() { n += 1; }
            let id = format!("v{n}");
            let mut meta_idx = idx;
            meta_idx.upsert(SlotMeta { id: id.clone(), name: format!("虚拟槽 {n}"), exes: vec![] });
            let _ = meta_idx.save();
            let _ = SlotConfig { name: format!("虚拟槽 {n}"), ..Default::default() }.save(&id);
            let _ = client::call("VSRELOAD");
            load_vs_into_editor(&ui, &id);
            refresh_vslots(&ui);
            show_toast(weak.clone(), format!("已新建 {id}，填好程序名后点保存"));
        }
    });

    let weak = ui.as_weak();
    ui.on_vs_save(move || {
        if let Some(ui) = weak.upgrade() {
            match save_vs_from_editor(&ui) {
                Ok(msg) => {
                    show_toast(weak.clone(), msg);
                    refresh_vslots(&ui);
                }
                Err(e) => show_toast(weak.clone(), e),
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_vs_delete(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_vs_edit_id().to_string();
        if id.is_empty() { return; }
        let mut idx = VsIndex::load();
        idx.remove(&id);
        let _ = idx.save();
        let _ = std::fs::remove_file(paths::vslot_path(&id));
        let _ = client::call("VSRELOAD");
        ui.set_vs_edit_id("".into());
        ui.set_vs_edit_name("".into());
        ui.set_vs_edit_exes("".into());
        refresh_vslots(&ui);
        show_toast(weak.clone(), format!("已删除虚拟槽 {id}"));
    });

    let weak = ui.as_weak();
    ui.on_vs_apply(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_vs_edit_id().to_string();
        if id.is_empty() { return; }
        // 套用走 agent（它持有设备操作与记账），后台线程避免卡 UI。
        let weak2 = weak.clone();
        std::thread::spawn(move || {
            let result = client::call(&format!("VSAPPLY {id}"));
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak2.upgrade() {
                    match result {
                        Ok(msg) => show_toast(ui.as_weak(), msg.trim_start_matches("OK ").to_string()),
                        Err(e) => show_toast(ui.as_weak(), format!("套用失败：{e}")),
                    }
                    refresh_vslots(&ui);
                }
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_vs_set_enabled(move |on| {
        if let Some(ui) = weak.upgrade() {
            let mut idx = VsIndex::load();
            idx.enabled = on;
            if let Err(e) = idx.save() {
                show_toast(weak.clone(), format!("保存失败：{e}"));
                return;
            }
            let _ = client::call("VSRELOAD");
            ui.set_vs_enabled(on);
            // 切回离线时若停在在线专属页（自动切换），跳回概览，避免停在已隐藏的页面。
            if !on && ui.get_page() == 3 {
                ui.set_page(0);
            }
            show_toast(weak.clone(), if on {
                "已开启在线模式 · 自动切换 / 无限虚拟槽 / 三区灯效已解锁"
            } else {
                "已切回离线模式 · 仅使用板载 5 槽"
            }.into());
        }
    });

    spawn_agent_refresh(ui.as_weak());

    // ── 宏编辑 ──
    let weak = ui.as_weak();
    ui.on_open_macro(move |id| {
        let weak2 = weak.clone();
        // 读 Flash 要几十毫秒，别卡住界面线程。
        std::thread::spawn(move || {
            let result = (|| -> Result<(u16, Vec<Step>), String> {
                let m = Mouse::open().map_err(|e| e.to_string())?;
                let data = m.macro_read(id as u16).map_err(|e| e.to_string())?;
                let steps = macro_codec::decode_steps(&data).ok_or_else(|| {
                    "这个宏里有本程序无法安全表达的动作（按住不放、未知操作码）。\
                     为免一保存就把它改坏，这里不提供编辑。"
                        .to_string()
                })?;
                Ok((id as u16, steps))
            })();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak2.upgrade() {
                    match result {
                        Ok((id, steps)) => {
                            *MACRO_EDIT.lock().unwrap() = Some((id, steps));
                            ui.set_step_sel(-1);
                            ui.set_step_arg(SharedString::from(""));
                            publish_macro(&ui);
                        }
                        Err(e) => show_toast(ui.as_weak(), format!("打不开：{e}")),
                    }
                }
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_close_macro(move || {
        *MACRO_EDIT.lock().unwrap() = None;
        if let Some(ui) = weak.upgrade() {
            publish_macro(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_pick_step(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.set_step_sel(index);
            // 把这一步的参数装进编辑框，配合「改选中」就能原地修改。
            let guard = MACRO_EDIT.lock().unwrap();
            if let Some((_, steps)) = guard.as_ref() {
                if let Some(s) = steps.get(index as usize) {
                    ui.set_step_kind(step_kind(s));
                    ui.set_step_arg(SharedString::from(step_arg_of(s)));
                }
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_add_step(move |kind, arg| {
        let Some(ui) = weak.upgrade() else { return };
        let step = match make_step(kind, arg.as_str()) {
            Ok(s) => s,
            Err(e) => {
                show_toast(ui.as_weak(), e);
                return;
            }
        };
        match edit_macro(&ui, move |steps| {
            steps.push(step);
            Ok(())
        }) {
            Ok(()) => {
                // 故意不清空参数框：连续加同类步骤时直接再按一次「添加步骤」更顺手。
                ui.set_step_sel(-1);
            }
            Err(e) => show_toast(ui.as_weak(), e),
        }
    });

    let weak = ui.as_weak();
    ui.on_update_step(move |index, kind, arg| {
        let Some(ui) = weak.upgrade() else { return };
        let step = match make_step(kind, arg.as_str()) {
            Ok(s) => s,
            Err(e) => {
                show_toast(ui.as_weak(), e);
                return;
            }
        };
        let i = index as usize;
        if let Err(e) = edit_macro(&ui, move |steps| {
            let Some(slot) = steps.get_mut(i) else {
                return Err("这一步已经不在了".to_string());
            };
            *slot = step;
            Ok(())
        }) {
            show_toast(ui.as_weak(), e);
        }
    });

    let weak = ui.as_weak();
    ui.on_remove_step(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let i = index as usize;
        if let Err(e) = edit_macro(&ui, move |steps| {
            if i >= steps.len() {
                return Err("这一步已经不在了".to_string());
            }
            steps.remove(i);
            Ok(())
        }) {
            show_toast(ui.as_weak(), e);
            return;
        }
        ui.set_step_sel(-1);
    });

    let weak = ui.as_weak();
    ui.on_move_step(move |index, delta| {
        let Some(ui) = weak.upgrade() else { return };
        let i = index as usize;
        let d = delta as isize;
        if let Err(e) = edit_macro(&ui, move |steps| {
            let j = i as isize + d;
            if i >= steps.len() || j < 0 || j as usize >= steps.len() {
                return Err("已经到头了".to_string());
            }
            steps.swap(i, j as usize);
            Ok(())
        }) {
            show_toast(ui.as_weak(), e);
            return;
        }
        // 选中态跟着走，用户连按上移时不会突然丢掉焦点。
        let j = (i as isize + d).max(0);
        ui.set_step_sel(j as i32);
    });

    let weak = ui.as_weak();
    ui.on_write_macro(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        let (mid, body) = {
            let guard = MACRO_EDIT.lock().unwrap();
            match guard.as_ref() {
                Some((mid, steps)) => match macro_codec::encode_steps(steps) {
                    Ok(b) => (*mid, b),
                    Err(e) => {
                        drop(guard);
                        show_toast(ui.as_weak(), format!("编码失败：{e}"));
                        return;
                    }
                },
                None => {
                    drop(guard);
                    show_toast(ui.as_weak(), "没有打开任何宏".into());
                    return;
                }
            }
        };
        let _ = id;
        ui.set_busy(true);
        let weak2 = ui.as_weak();
        std::thread::spawn(move || {
            let outcome = (|| -> Result<String, String> {
                let m = Mouse::open().map_err(|e| e.to_string())?;
                m.macro_write(mid, &body).map_err(|e| e.to_string())?;
                razer_ipc::known::remember(mid);
                // 写完必须读回来比：Flash 写入失败的返回码不一定准。
                let back = m.macro_read(mid).map_err(|e| e.to_string())?;
                if back != body {
                    return Err(format!(
                        "读回不一致（写 {} B，读回 {} B）",
                        body.len(),
                        back.len()
                    ));
                }
                Ok(format!(
                    "宏 0x{mid:04X} 已写入 {} B 并校验通过",
                    body.len()
                ))
            })();
            let payload = collect();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak2.upgrade() {
                    apply(&ui, payload);
                    match outcome {
                        Ok(msg) => show_toast(ui.as_weak(), msg),
                        Err(e) => show_toast(ui.as_weak(), format!("写入失败：{e}")),
                    }
                }
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_delete_macro(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        let mid = id as u16;
        ui.set_busy(true);
        let weak2 = ui.as_weak();
        std::thread::spawn(move || {
            let outcome = (|| -> Result<String, String> {
                let m = Mouse::open().map_err(|e| e.to_string())?;
                m.macro_delete(mid).map_err(|e| e.to_string())?;
                razer_ipc::known::forget(mid);
                Ok(format!("宏 0x{mid:04X} 已删除"))
            })();
            // 删掉的正好是打开的那条就顺手关掉编辑区。
            if MACRO_EDIT.lock().unwrap().as_ref().map(|(m, _)| *m) == Some(mid) {
                *MACRO_EDIT.lock().unwrap() = None;
            }
            let payload = collect();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak2.upgrade() {
                    apply(&ui, payload);
                    publish_macro(&ui);
                    match outcome {
                        Ok(msg) => show_toast(ui.as_weak(), msg),
                        Err(e) => show_toast(ui.as_weak(), format!("删除失败：{e}")),
                    }
                }
            });
        });
    });

    // ── 扫描 Flash 里已有的宏 ──
    // 设备只报总数不报 ID，只能按区间一个个问。放在线程里跑，界面上显示「读取中」。
    let weak = ui.as_weak();
    ui.on_scan_macros(move || {
        let Some(ui) = weak.upgrade() else { return };
        ui.set_busy(true);
        let weak2 = ui.as_weak();
        std::thread::spawn(move || {
            let outcome = (|| -> Result<String, String> {
                let m = Mouse::open().map_err(|e| e.to_string())?;
                let count = m.macro_count().unwrap_or(0) as usize;
                let found = m.macro_scan(0x0001, 0x0400, count);
                razer_ipc::known::remember_all(&found);
                Ok(format!("扫描完成，找到 {} 个宏", found.len()))
            })();
            let payload = collect();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak2.upgrade() {
                    apply(&ui, payload);
                    match outcome {
                        Ok(msg) => show_toast(ui.as_weak(), msg),
                        Err(e) => show_toast(ui.as_weak(), format!("扫描失败：{e}")),
                    }
                }
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_new_macro(move |hex| {        let Some(ui) = weak.upgrade() else { return };
        let Some(id) = razer_core::parse_macro_id(hex.as_str()) else {
            show_toast(ui.as_weak(), "宏 ID 要写成 0x0101 这样的十六进制".into());
            return;
        };
        if id == 0 {
            show_toast(ui.as_weak(), "0x0000 不是合法宏 ID".into());
            return;
        }
        *MACRO_EDIT.lock().unwrap() = Some((id, Vec::new()));
        ui.set_new_macro_id(SharedString::from(""));
        ui.set_step_sel(-1);
        ui.set_step_arg(SharedString::from(""));
        publish_macro(&ui);
        show_toast(
            ui.as_weak(),
            format!("新建宏 0x{id:04X}，加完步骤后点「写入 Flash」"),
        );
    });

    // `--shot <file.bmp>`：等数据读完之后把窗口渲染结果存下来再退出。
    // 开发期用它核对界面，比抓屏可靠——不受多显示器 DPI 和窗口层叠影响。
    if let Some(i) = args.iter().position(|a| a == "--shot") {
        if let Some(path) = args.get(i + 1).cloned() {
            let weak = ui.as_weak();
            let start = Instant::now();
            let timer = Box::leak(Box::new(Timer::default()));
            timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
                let Some(ui) = weak.upgrade() else { return };
                let ready = !ui.get_busy() || start.elapsed() > Duration::from_secs(10);
                if ready && start.elapsed() > Duration::from_millis(900) {
                    if let Ok(buf) = ui.window().take_snapshot() {
                        match write_bmp(&path, &buf) {
                            Ok(()) => println!("已写出 {path} ({}x{})", buf.width(), buf.height()),
                            Err(e) => eprintln!("写截图失败：{e}"),
                        }
                    }
                    std::process::exit(0);
                }
            });
        }
    }

    // ── 设置页：备份 / 恢复 / 打开目录 / 开机启动 ──
    let weak = ui.as_weak();
    ui.on_backup_save(move || {
        spawn_op(weak.clone(), move |m| {
            let rules = std::fs::read_to_string(paths::rules_path()).unwrap_or_default();
            let known =
                std::fs::read_to_string(paths::known_macros_path()).unwrap_or_default();
            let text = m.export_config(&rules, &known, razer_ipc::ledbak::load()).map_err(|e| e.to_string())?;
            let dir = paths::backup_dir();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("latest.txt"), &text).map_err(|e| e.to_string())?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = std::fs::write(dir.join(format!("snapshot-{stamp}.txt")), &text);
            Ok(format!(
                "已备份 {} 行到 backups/latest.txt",
                text.lines().count()
            ))
        });
        spawn_settings_refresh(weak.clone(), 1500);
    });

    let weak = ui.as_weak();
    ui.on_backup_restore(move || {
        spawn_op(weak.clone(), move |m| {
            let text = std::fs::read_to_string(backup_latest())
                .map_err(|e| format!("读取备份失败：{e}"))?;
            let (rules, known, applied) =
                m.restore_config(&text).map_err(|e| e.to_string())?;
            std::fs::write(paths::rules_path(), rules).map_err(|e| e.to_string())?;
            std::fs::write(paths::known_macros_path(), known)
                .map_err(|e| e.to_string())?;
            let _ = client::call("RELOAD");
            Ok(format!("已恢复 {applied} 项设备设置，逐项读回校验通过"))
        });
        spawn_settings_refresh(weak.clone(), 1500);
    });

    ui.on_backup_open_dir(move || {
        let dir = paths::backup_dir();
        let _ = std::fs::create_dir_all(&dir);
        // 走 Shell API 打开文件夹，不显式启动任何外部可执行程序。
        let verb = to_wide("open");
        let target = to_wide(&dir.to_string_lossy());
        unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            );
        }
    });

    let weak = ui.as_weak();
    ui.on_toggle_autostart(move || {
        let Some(ui) = weak.upgrade() else { return };
        let want = !ui.get_autostart_on();
        match autostart_set(want) {
            Ok(()) => {
                ui.set_autostart_on(want);
                show_toast(
                    ui.as_weak(),
                    if want {
                        "已开启开机启动后台服务".into()
                    } else {
                        "已关闭开机启动".into()
                    },
                );
            }
            Err(e) => show_toast(ui.as_weak(), format!("设置开机启动失败：{e}")),
        }
    });

    spawn_settings_refresh(ui.as_weak(), 0);

    ui.run()
}

/// 把 Slint 的快照写成 32 位 BMP。
///
/// 用 BMP 而不是 PNG，是因为 BMP 头部固定 54 字节、不需要任何压缩库；截图只是
/// 开发期工具，不值得为它引入一个编码器依赖。需要 PNG 时用 Python 转一下即可。
fn write_bmp(
    path: &str,
    buf: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) -> std::io::Result<()> {
    let w = buf.width();
    let h = buf.height();
    let data_size = (w * 4 * h) as u32;
    let file_size = 54 + data_size;

    let mut out = Vec::with_capacity(file_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    // BMP 是自下而上存储，且通道顺序是 BGRA。
    let px = buf.as_slice();
    for y in (0..h).rev() {
        let row = &px[(y * w) as usize..((y + 1) * w) as usize];
        for p in row {
            out.push(p.b);
            out.push(p.g);
            out.push(p.r);
            out.push(255);
        }
    }
    std::fs::write(path, out)
}

fn region_label(region: u8) -> &'static str {
    match region {
        cmd::region::WHEEL => "滚轮",
        cmd::region::LOGO => "Logo",
        cmd::region::STRIP => "底边灯带",
        _ => "全部区域",
    }
}
