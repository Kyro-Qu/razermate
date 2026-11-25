//! `razerctl` — command line control for the Razer Basilisk V3.
//!
//! Deliberately dependency free: no clap, no serde. Parsing a dozen verbs by
//! hand is cheaper than 300 KB of crates and keeps startup in the microseconds.

use std::process::ExitCode;

use razer_core::{button_name, Mouse, Result};
use razer_protocol::commands::{self as cmd, region as reg, Binding, FlashInfo};
use razer_protocol::keys::{button as btn, usage};
use razer_protocol::macro_codec;
use razer_protocol::report::Report;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    };
    ExitCode::from(code)
}

fn run(args: &[String]) -> Result<()> {
    let (verb, rest) = match args.split_first() {
        Some((v, r)) => (v.as_str(), r),
        None => {
            print_help();
            return Ok(());
        }
    };

    match verb {
        "help" | "-h" | "--help" => {
            print_help();
            return Ok(());
        }
        "paths" => {
            for p in razer_hid::list_paths()? {
                println!("{p}");
            }
            return Ok(());
        }
        "hexdump" => {
            // Build one report without touching the device; used by the golden
            // test that compares our bytes against the Python implementation.
            let nums: Vec<u32> = rest
                .iter()
                .filter_map(|s| {
                    let t = s.trim();
                    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                        u32::from_str_radix(h, 16).ok()
                    } else {
                        t.parse().ok()
                    }
                })
                .collect();
            if nums.len() < 3 {
                eprintln!("usage: razerctl hexdump <class> <cmd> <size> [args...]");
                return Ok(());
            }
            let args: Vec<u8> = nums[3..].iter().map(|v| *v as u8).collect();
            let report =
                razer_protocol::Report::request(nums[0] as u8, nums[1] as u8, nums[2] as u8, &args);
            println!("{}", hex(&report.to_bytes()));
            return Ok(());
        }
        _ => {}
    }

    let mouse = Mouse::open()?;

    match verb {
        "info" => info(&mouse),
        "check" => check(&mouse),
        "flash" => {
            let f = mouse.flash()?;
            print_flash(&f);
            Ok(())
        }
        "mode" => {
            println!("{}", describe_mode(mouse.mode()?));
            Ok(())
        }
        "normal" => {
            let after = mouse.ensure_normal()?;
            println!("mode -> {after}");
            if after == 0 {
                println!("onboard bindings and macros apply");
            } else {
                println!("still not normal; unplug the mouse once");
            }
            Ok(())
        }
        "slot" => slot(&mouse, rest),
        "bind" => bind(&mouse, rest),
        "binding" => {
            let id = rest.first().and_then(|s| btn::parse(s)).unwrap_or(0);
            if id == 0 {
                eprintln!("usage: razerctl binding <button> [--slot N]");
                return Ok(());
            }
            let profile = rest
                .iter()
                .position(|s| s == "--slot")
                .and_then(|i| rest.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(mouse.active_profile()?);
            let b = mouse.binding(profile, id, cmd::layer::NORMAL)?;
            println!(
                "slot {profile}  {}: {}  (fn={} data={:02x?})",
                button_name(id),
                describe_binding(b),
                b.fn_id,
                &b.data[..b.fn_size as usize]
            );
            Ok(())
        }
        "macro" => macro_cmd(&mouse, rest),
        "led" => led(&mouse, rest),
        "onboard" => onboard(&mouse, rest),
        "bright" => bright(&mouse, rest),
        "dpi" => dpi(&mouse, rest),
        "raw" => raw(&mouse, rest),
        "scan-bindings" => scan_bindings(&mouse, rest),
        "bench-led" => bench_led(&mouse, rest),
        "backup" => backup_cmd(&mouse, rest),
        "selftest" => selftest(&mouse),
        other => {
            eprintln!("unknown command {other:?}\n");
            print_help();
            Ok(())
        }
    }
}

fn info(mouse: &Mouse) -> Result<()> {
    let (a, b, c) = mouse.firmware()?;
    let flash = mouse.flash()?;
    let active = mouse.active_profile()?;
    println!("Razer Basilisk V3   fw v{a}.{b}.{c}   1532:0099");
    println!("mode        {}", describe_mode(mouse.mode()?));
    println!("active slot {active}");
    println!(
        "flash       {} B total, {} B available, {} B recycled",
        flash.total, flash.available, flash.recycled
    );
    println!(
        "            {} B immediately usable ({} pages)",
        flash.immediately_usable(),
        flash.immediately_usable() / flash.page_size()
    );
    println!("macros      {}", mouse.macro_count()?);
    Ok(())
}

fn check(mouse: &Mouse) -> Result<()> {
    let mode = mouse.mode()?;
    println!("mode {}", describe_mode(mode));
    if mode != 0 {
        println!("  ! driver mode: onboard bindings do NOT apply; run \"razerctl normal\"");
    }
    let active = mouse.active_profile()?;
    println!("active slot {active}");
    for id in mouse.profile_list()? {
        let mark = if id == active { "  <- active" } else { "" };
        println!("  slot {id}{mark}");
    }
    println!("\nbindings of slot {active}:");
    for (id, name) in btn::ALL {
        // Wheel up/down are not rebindable in the same table.
        if *id == btn::WHEEL_UP || *id == btn::WHEEL_DOWN {
            continue;
        }
        match mouse.binding(active, *id, cmd::layer::NORMAL) {
            Ok(b) => println!("  {:<12} {}", name, describe_binding(b)),
            Err(err) => println!("  {:<12} ! {err}", name),
        }
    }
    Ok(())
}

fn slot(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let active = mouse.active_profile()?;
    match rest.first().map(String::as_str) {
        None | Some("list") => {
            for id in mouse.profile_list()? {
                let led = mouse.lighting(id, reg::STRIP).ok();
                let bright = mouse.brightness(id, reg::STRIP).unwrap_or(0);
                let mark = if id == active { "  <- active" } else { "" };
                let light = led.map(|l| l.describe()).unwrap_or_else(|| "?".into());
                println!(
                    "slot {id}  {:<18} {:<26} {:>3}%{}",
                    razer_core::slot_colour_name(id),
                    light,
                    bright as u16 * 100 / 255,
                    mark
                );
            }
            Ok(())
        }
        Some("show") => {
            let id = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(active);
            println!("slot {id} ({})", razer_core::slot_colour_name(id));
            for (region, name) in [
                (reg::WHEEL, "scroll"),
                (reg::LOGO, "logo"),
                (reg::STRIP, "strip"),
            ] {
                let l = mouse.lighting(id, region).ok();
                let b = mouse.brightness(id, region).unwrap_or(0);
                println!(
                    "  {name:<8} {:<28} {:>3}%",
                    l.map(|l| l.describe()).unwrap_or_else(|| "?".into()),
                    b as u16 * 100 / 255
                );
            }
            Ok(())
        }
        Some("use") => {
            let id: u8 = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(active);
            let got = mouse.set_active_profile(id)?;
            println!("active slot {got}");
            Ok(())
        }
        Some(other) => {
            eprintln!("slot: unknown subcommand {other:?}");
            Ok(())
        }
    }
}

fn bind(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let (button_spec, action) = match (rest.first(), rest.get(1)) {
        (Some(b), Some(a)) => (b.as_str(), a.as_str()),
        _ => {
            eprintln!("usage: razerctl bind <button> <key:ctrl+w | macro:0x0101:x3>");
            return Ok(());
        }
    };
    let button = btn::parse(button_spec).unwrap_or(0);
    if button == 0 {
        eprintln!("unknown button {button_spec:?}");
        return Ok(());
    }
    let profile = rest
        .iter()
        .position(|s| s == "--slot")
        .and_then(|i| rest.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(mouse.active_profile()?);
    let binding = match razer_core::parse_action(action) {
        Some(b) => b,
        None => {
            eprintln!("unknown action {action:?}");
            return Ok(());
        }
    };
    let got = mouse.set_binding(profile, button, cmd::layer::NORMAL, binding)?;
    println!("slot {profile}  {}: {}", button_name(button), describe_binding(got));
    Ok(())
}

fn macro_cmd(mouse: &Mouse, rest: &[String]) -> Result<()> {
    match rest.first().map(String::as_str) {
        None | Some("list") => {
            let count = mouse.macro_count()? as usize;
            // 先查登记表（快），凑不够数再扫一遍最常见的 ID 区间并记下来。
            let mut ids = razer_ipc::known::retain_existing(|id| {
                mouse.macro_size(id).map(|s| s > 0).unwrap_or(false)
            });
            if ids.len() < count {
                let found = mouse.macro_scan(0x0001, 0x0400, count);
                razer_ipc::known::remember_all(&found);
                ids.extend(found);
                ids.sort_unstable();
                ids.dedup();
            }
            if ids.is_empty() {
                println!("no macros stored");
                return Ok(());
            }
            for id in ids {
                let data = mouse.macro_read(id)?;
                println!("  {id:04x}  {:4} B  {}", data.len(), macro_codec::decode(&data));
            }
            Ok(())
        }
        Some("show") => {
            let id = parse_id(rest.get(1)).unwrap_or(0);
            let data = mouse.macro_read(id)?;
            println!("id {id:04x}  {} bytes", data.len());
            println!("  hex: {}", hex(&data));
            println!("  dec: {}", macro_codec::decode(&data));
            Ok(())
        }
        Some("add") => {
            let id = match parse_id(rest.get(1)) {
                Some(v) => v,
                None => {
                    eprintln!("usage: razerctl macro add <id> <step, step, ...>");
                    return Ok(());
                }
            };
            let spec = rest[2..].join(",");
            let steps = match macro_codec::parse_steps(&spec) {
                Ok(s) => s,
                Err(err) => {
                    eprintln!("{err}");
                    return Ok(());
                }
            };
            let body = match macro_codec::encode_steps(&steps) {
                Ok(b) => b,
                Err(err) => {
                    eprintln!("{err}");
                    return Ok(());
                }
            };
            mouse.macro_write(id, &body)?;
            razer_ipc::known::remember(id);
            let back = mouse.macro_read(id)?;
            let verdict = if back == body { "verified" } else { "MISMATCH" };
            println!("macro {id:04x}: wrote {} B, {verdict}", body.len());
            println!("  {}", macro_codec::decode(&back));
            Ok(())
        }
        Some("del") | Some("delete") => {
            let id = parse_id(rest.get(1)).unwrap_or(0);
            mouse.macro_delete(id)?;
            razer_ipc::known::forget(id);
            println!("deleted {id:04x}");
            Ok(())
        }
        Some("scan") => {
            let from = parse_id(rest.get(1)).unwrap_or(1);
            let to = parse_id(rest.get(2)).unwrap_or(0x0400);
            println!("scaning 0x{from:04X}..=0x{to:04X} …");
            let found = mouse.macro_scan(from, to, 0);
            razer_ipc::known::remember_all(&found);
            if found.is_empty() {
                println!("nothing found in that range");
            } else {
                for id in &found {
                    let size = mouse.macro_size(*id).unwrap_or(0);
                    let data = mouse.macro_read(*id).unwrap_or_default();
                    println!("  0x{id:04X}  {size:4} B  {}", macro_codec::decode(&data));
                }
                println!("{} macro(s), 已记进登记表", found.len());
            }
            Ok(())
        }
        Some(other) => {
            eprintln!("macro: unknown subcommand {other:?}");
            Ok(())
        }
    }
}

fn led(mouse: &Mouse, rest: &[String]) -> Result<()> {
    if rest.first().map(String::as_str) == Some("preview") {
        return led_preview(mouse, &rest[1..]);
    }
    let (zone, colour) = match (rest.first(), rest.get(1)) {
        (Some(z), Some(c)) => (z.as_str(), c.as_str()),
        _ => {
            eprintln!("usage: razerctl led <backlight|logo|scroll|all> <colour>");
            return Ok(());
        }
    };
    let rgb = match parse_colour(colour) {
        Some(c) => c,
        None => {
            eprintln!("unknown colour {colour:?}");
            return Ok(());
        }
    };
    let profile = mouse.active_profile()?;
    for region in regions(zone) {
        let l = mouse.set_static(profile, region, rgb)?;
        println!("slot {profile} region 0x{region:02x}: {}", l.describe());
    }
    Ok(())
}

/// `led preview` —— 逐颗灯带的运行时预览。
///
/// 实测 11 颗灯约 7 ms/帧（142 fps），软件动画技术上可行；但 `set_custom`
/// 不写 Flash、持续刷又会占 CPU，所以只给一次性图案和 2 秒短动画，
/// 动画结束自动还原，另有 `restore` 手动还原。
fn led_preview(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let mode = rest.first().map(String::as_str).unwrap_or("rainbow");
    let profile = mouse.active_profile()?;
    let region = cmd::region::STRIP;
    let n = cmd::region::STRIP_LEDS as usize;

    // 预览前先把**板载**灯效存下来，restore 才有得还原（可能隔天、换进程）。
    // 连续点预览时设备已经停在 CUSTOM 帧上，这时绝不能再覆盖备份，否则就把
    // 临时帧记成了板载效果，还原后会写死一颗彩虹色。
    let backup = |old: &cmd::Lighting| {
        let rgb = old.colors.first().copied().unwrap_or((0, 0, 0));
        let _ = razer_ipc::ledbak::save(old.effect, old.mode, old.speed, rgb);
    };
    let board = || -> Option<cmd::Lighting> {
        razer_ipc::ledbak::load().map(|(effect, mode, speed, rgb)| cmd::Lighting {
            effect,
            mode,
            speed,
            colors: vec![rgb],
        })
    };

    match mode {
        "rainbow" | "gradient" => {
            let now = mouse.lighting(profile, region)?;
            if now.effect != cmd::effect::CUSTOM {
                backup(&now);
            }
            let base = board()
                .and_then(|l| l.colors.first().copied())
                .or_else(|| now.colors.first().copied())
                .unwrap_or((68, 214, 44));
            let colours = if mode == "rainbow" {
                razer_core::led_rainbow(n, 0)
            } else {
                razer_core::led_gradient(n, base)
            };
            mouse.set_custom(profile, region, &colours)?;
            println!("预览 {}：{} 颗灯（运行时，不写 Flash）", mode, colours.len());
            println!("  {}", colours.iter().map(|c| format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2)).collect::<Vec<_>>().join(" "));
            println!("用 `razerctl led preview restore` 还原");
        }
        "animate" => {
            let frames = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(60usize);
            let now = mouse.lighting(profile, region)?;
            if now.effect != cmd::effect::CUSTOM {
                backup(&now);
            }
            // 结束时要还原的是「进预览之前的板载效果」，不是当前这帧 CUSTOM。
            let target = match board() {
                Some(l) => l,
                None if now.effect == cmd::effect::CUSTOM => {
                    eprintln!("当前停在逐灯预览且没有板载灯效备份，请先执行 led preview restore");
                    return Ok(());
                }
                None => now,
            };
            let (ok, secs) = mouse.led_animate(profile, region, &target, frames)?;
            println!(
                "动画 {} 帧 / {:.2} s（{:.1} fps），已自动还原",
                ok,
                secs,
                ok as f64 / secs.max(0.001)
            );
            razer_ipc::ledbak::clear();
        }
        "restore" => {
            let Some((effect, mode, speed, rgb)) = razer_ipc::ledbak::load() else {
                eprintln!("没有可还原的灯效备份");
                return Ok(());
            };
            let old = cmd::Lighting {
                effect,
                mode,
                speed,
                colors: vec![rgb],
            };
            let back = mouse.led_restore(profile, region, &old)?;
            println!("已还原灯效：{}", razer_core::light_detail_zh(&back));
            razer_ipc::ledbak::clear();
        }
        other => {
            eprintln!("usage: razerctl led preview <rainbow|gradient|animate [frames]|restore>");
            eprintln!("unknown mode {other:?}");
        }
    }
    Ok(())
}

fn onboard(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let (zone, effect) = match (rest.first(), rest.get(1)) {
        (Some(z), Some(e)) => (z.as_str(), e.as_str()),
        _ => {
            eprintln!("usage: razerctl onboard <zone|all> <static #rrggbb | spectrum | wave dir speed>");
            return Ok(());
        }
    };
    let profile = rest
        .iter()
        .position(|s| s == "--slot")
        .and_then(|i| rest.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(mouse.active_profile()?);

    for region in regions(zone) {
        let l = match effect {
            "static" => {
                let rgb = rest.get(2).and_then(|c| parse_colour(c)).unwrap_or((255, 0, 0));
                mouse.set_static(profile, region, rgb)?
            }
            "spectrum" => mouse.set_spectrum(profile, region)?,
            "wave" => {
                let dir = rest.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
                let speed = rest.get(3).and_then(|s| parse_speed(s)).unwrap_or(0x40);
                mouse.set_wave(profile, region, dir, speed)?
            }
            other => {
                eprintln!("unknown effect {other:?}");
                return Ok(());
            }
        };
        println!("slot {profile} region 0x{region:02x}: {}", l.describe());
    }
    Ok(())
}

fn bright(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let pct: u8 = rest
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    let profile = rest
        .iter()
        .position(|s| s == "--slot")
        .and_then(|i| rest.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(mouse.active_profile()?);
    let got = mouse.set_brightness(profile, reg::ALL, pct)?;
    println!("slot {profile} brightness {got}%");
    Ok(())
}

fn dpi(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let profile = rest
        .iter()
        .position(|s| s == "--slot")
        .and_then(|i| rest.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(mouse.active_profile()?);

    match rest.first().map(String::as_str) {
        None | Some("show") => {
            let (x, y) = mouse.dpi(profile)?;
            let hz = mouse.polling_hz(profile)?;
            println!("slot {profile}  dpi {x} x {y}  poll {hz} Hz");
            let stages = mouse.dpi_stages(profile)?;
            for (i, (idx, sx, sy)) in stages.stages.iter().enumerate() {
                let mark = if *idx == stages.active { "*" } else { " " };
                println!("  {mark} stage {i} (id {idx})  {sx} x {sy}");
            }
            Ok(())
        }
        Some("set") => {
            let x: u16 = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let y: u16 = rest.get(2).and_then(|s| s.parse().ok()).unwrap_or(x);
            if !(100..=26000).contains(&x) || !(100..=26000).contains(&y) {
                eprintln!("dpi: 取值范围 100..26000");
                return Ok(());
            }
            let (gx, gy) = mouse.set_dpi(profile, x, y)?;
            println!("slot {profile}  dpi -> {gx} x {gy}");
            Ok(())
        }
        Some("poll") => {
            let hz: u16 = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(1000);
            let got = mouse.set_polling_hz(profile, hz)?;
            println!("slot {profile}  poll -> {got} Hz");
            Ok(())
        }
        Some(other) => {
            eprintln!("dpi: unknown subcommand {other:?} (show | set <x> [y] | poll <hz>)");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// `raw <class> <cmd> <size> [args...]` —— 发一条报文并把应答完整打出来。
///
/// 用途：把 PROTOCOL.md §8 里那些「待补验证」的命令码逐个试出来（DPI、回报率、
/// 滚轮模式…）。参数接受十进制或 0x 前缀十六进制。
fn raw(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let nums: Vec<u32> = rest
        .iter()
        .filter_map(|s| {
            let t = s.trim();
            match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(h) => u32::from_str_radix(h, 16).ok(),
                None => t.parse().ok(),
            }
        })
        .collect();
    if nums.len() < 3 {
        eprintln!("usage: razerctl raw <class> <cmd> <size> [args...]");
        return Ok(());
    }
    let args: Vec<u8> = nums[3..].iter().map(|v| *v as u8).collect();
    let req = Report::request(nums[0] as u8, nums[1] as u8, nums[2] as u8, &args);
    let r = mouse.call_raw(&req)?;
    println!("status={:?}", r.status);
    println!("reply  {}", hex(&r.to_bytes()));
    let a = r.reply();
    println!("data   {} bytes: {:02x?}", a.len(), a);
    Ok(())
}

/// `scan-bindings [slot] [from] [to]` —— 扫一段按钮 id，看哪些真的存在。
///
/// 这台设备的按钮 id 不连续（tilt 是 52/53、DPI 是 96），逐个试比查表可靠。
fn scan_bindings(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let profile = rest
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(mouse.active_profile()?);
    let from: u8 = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let to: u8 = rest.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);

    println!("slot {profile}, button {from}..={to}");
    for id in from..=to {
        match mouse.binding(profile, id, cmd::layer::NORMAL) {
            Ok(b) => println!(
                "  {:>3}  fn={:<3} {:02x?}  {}",
                id,
                b.fn_id,
                &b.data[..b.fn_size as usize],
                describe_binding(b)
            ),
            Err(_) => {}
        }
    }
    Ok(())
}

/// `bench-led [帧数]` —— 测逐颗 LED 连刷的真实帧率。
///
/// 这决定「渐变/呼吸动画」值不值得做：每帧要走一次完整的 feature report
/// （HID 层往返 + 设备端解析），到底能到多少帧只能实测。逐颗写入不落 Flash，
/// 所以跑多久都不消耗页；跑完把原来的效果原样写回去。
///
/// 门槛按 PLAN.md 定的：低于 20 fps 就不做软件动画，改用板载光谱/波浪。
fn bench_led(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let frames: usize = rest
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60)
        .clamp(5, 2000);
    let profile = mouse.active_profile()?;
    let region = cmd::region::STRIP;
    let n = cmd::region::STRIP_LEDS;

    let mut old = mouse.lighting(profile, region)?;
    if old.effect == cmd::effect::CUSTOM {
        // 设备停在预览帧上：先把板载灯效还原回去再跑基准，否则收尾会把
        // 预览帧的第一颗颜色写成永久静态色（实测踩过，#FF8800 就是这么来的）。
        match razer_ipc::ledbak::load() {
            Some((effect, mode, speed, rgb)) => {
                old = cmd::Lighting {
                    effect,
                    mode,
                    speed,
                    colors: vec![rgb],
                };
                mouse.led_restore(profile, region, &old)?;
            }
            None => {
                eprintln!("当前停在逐灯预览且没有板载灯效备份，请先执行 led preview restore");
                return Ok(());
            }
        }
    }
    println!(
        "当前灯效：{}；开始 {} 帧逐颗写入（{} 颗灯）",
        razer_core::light_detail_zh(&old),
        frames,
        n
    );

    let mut frames_ok = 0usize;
    let mut slowest = 0u128;
    let start = std::time::Instant::now();

    for f in 0..frames {
        // 一帧彩虹滚动：每颗灯相位不同，这样肉眼也能看出刷的是新帧。
        let colors: Vec<(u8, u8, u8)> = (0..n)
            .map(|i| {
                let hue = ((f * 8 + i * 23) % 360) as f32;
                razer_core::hsv_to_rgb(hue, 1.0, 1.0)
            })
            .collect();
        let t0 = std::time::Instant::now();
        match mouse.set_custom(profile, region, &colors) {
            Ok(()) => {
                frames_ok += 1;
                slowest = slowest.max(t0.elapsed().as_micros());
            }
            Err(e) => {
                eprintln!("第 {f} 帧失败：{e}");
                break;
            }
        }
    }

    let total = start.elapsed();
    let fps = if total.as_secs_f64() > 0.0 {
        frames_ok as f64 / total.as_secs_f64()
    } else {
        0.0
    };

    println!(
        "{} 帧成功 / {}   总耗时 {:.0} ms   平均 {:.2} ms/帧   {:.1} fps   最慢 {:.2} ms",
        frames_ok,
        frames,
        total.as_millis(),
        total.as_secs_f64() * 1000.0 / frames_ok.max(1) as f64,
        fps,
        slowest as f64 / 1000.0
    );
    println!(
        "结论：{}",
        if fps >= 20.0 {
            "≥ 20 fps，软件动画可行（但会持续占 CPU，优先用板载效果）"
        } else {
            "不足 20 fps，不做软件动画，改用板载光谱/波浪"
        }
    );

    // 还原：逐颗是运行时效果，不还原的话鼠标会一直停在这帧上。
    let back = mouse.led_restore(profile, region, &old)?;
    println!("已还原灯效：{}", razer_core::light_detail_zh(&back));
    Ok(())
}


/// `backup` —— 纯文本配置快照，可读可 diff。
fn backup_cmd(mouse: &Mouse, rest: &[String]) -> Result<()> {
    let action = rest.first().map(String::as_str).unwrap_or("show");
    let dir = razer_ipc::paths::backup_dir();
    std::fs::create_dir_all(&dir)?;
    let latest = dir.join("latest.txt");

    let read_rules = || {
        std::fs::read_to_string(razer_ipc::paths::rules_path()).unwrap_or_default()
    };
    let read_known = || {
        std::fs::read_to_string(razer_ipc::paths::known_macros_path()).unwrap_or_default()
    };

    match action {
        "show" => {
            print!("{}", mouse.export_config(&read_rules(), &read_known(), razer_ipc::ledbak::load())?);
        }
        "save" => {
            let text = mouse.export_config(&read_rules(), &read_known(), razer_ipc::ledbak::load())?;
            std::fs::write(&latest, &text)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = std::fs::write(dir.join(format!("snapshot-{stamp}.txt")), &text);
            println!("已备份 {} 行 → {}", text.lines().count(), latest.display());
        }
        "restore" => {
            let file = rest
                .get(1)
                .map(std::path::PathBuf::from)
                .unwrap_or(latest);
            let text = std::fs::read_to_string(&file)?;
            let (rules, known, applied) = mouse.restore_config(&text)?;
            std::fs::write(razer_ipc::paths::rules_path(), rules)?;
            std::fs::write(razer_ipc::paths::known_macros_path(), known)?;
            // agent 在跑就让它重新读规则，不在跑也不算错。
            let _ = razer_ipc::client::call("RELOAD");
            println!(
                "已恢复 {} 项设备设置（逐项读回校验），规则与宏登记表已还原",
                applied
            );
        }
        other => {
            eprintln!("usage: razerctl backup <show|save|restore [file]>");
            eprintln!("unknown action {other:?}");
        }
    }
    Ok(())
}

/// `selftest` —— 对每个可写项跑「读旧值 → 写新值 → 读回校验 → 还原 → 再校验」。
///
/// 这是唯一能证明「写真的生效了」的办法：设备会在 normal 模式下对某些写入返回
/// OK 却什么也不做，光看返回值会被骗。
///
/// 只碰当前生效槽，且每项结束都把原值写回去。注意每次写都会消耗 Flash 页，
/// 所以只在验收时跑，别放进日常脚本。
fn selftest(mouse: &Mouse) -> Result<()> {
    let profile = mouse.active_profile()?;
    println!("自检：当前 {} 号槽（{}）", profile, razer_core::slot_label_zh(profile));
    if mouse.mode()? != 0 {
        println!("  ! 设备不在正常模式，先切回 0");
        mouse.set_mode(0)?;
    }

    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut report = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  PASS  {name:<14} {detail}");
        } else {
            fail += 1;
            println!("  FAIL  {name:<14} {detail}");
        }
    };

    // ── 亮度 ──
    let old_raw = mouse.brightness(profile, reg::STRIP)?;
    let old_pct = (old_raw as u16 * 100 / 255) as u8;
    let target = if old_pct > 50 { 40 } else { 80 };
    match mouse.set_brightness(profile, reg::STRIP, target) {
        Ok(got) if got == target => {
            // 还原必须用原始值：百分比来回换算会掉 1 LSB。
            let back = mouse.set_brightness_raw(profile, reg::STRIP, old_raw).unwrap_or(255);
            report(
                "亮度",
                back == old_raw,
                format!("{old_pct}% → {target}% → 还原 {back}/255 ({})",
                    if back == old_raw { "精确" } else { "偏差" }),
            );
        }
        Ok(got) => {
            let _ = mouse.set_brightness_raw(profile, reg::STRIP, old_raw);
            report("亮度", false, format!("写 {target}% 读回 {got}%"));
        }
        Err(e) => {
            let _ = mouse.set_brightness_raw(profile, reg::STRIP, old_raw);
            report("亮度", false, format!("{e}"));
        }
    }

    // ── 静态色 ──
    let old_light = mouse.lighting(profile, reg::STRIP).ok();
    let old_rgb = old_light
        .as_ref()
        .and_then(|l| l.colors.first().copied())
        .unwrap_or((255, 255, 255));
    let test_rgb = if old_rgb == (0x11, 0x22, 0x33) { (0x44, 0x55, 0x66) } else { (0x11, 0x22, 0x33) };
    match mouse.set_static(profile, reg::STRIP, test_rgb) {
        Ok(l) if l.colors.first() == Some(&test_rgb) => {
            let back = mouse.set_static(profile, reg::STRIP, old_rgb).ok();
            let restored = back.as_ref().and_then(|l| l.colors.first().copied()) == Some(old_rgb);
            report(
                "静态色",
                restored,
                format!("#{:02X}{:02X}{:02X} → #{:02X}{:02X}{:02X} → 还原 {}",
                    old_rgb.0, old_rgb.1, old_rgb.2, test_rgb.0, test_rgb.1, test_rgb.2,
                    if restored { "OK" } else { "失败" }),
            );
        }
        Ok(_) => {
            let _ = mouse.set_static(profile, reg::STRIP, old_rgb);
            report("静态色", false, "写入后读回颜色不一致".into());
        }
        Err(e) => {
            let _ = mouse.set_static(profile, reg::STRIP, old_rgb);
            report("静态色", false, format!("{e}"));
        }
    }

    // ── 按键绑定（用 DPI 键做靶子，最不影响日常使用） ──
    let target_button = btn::CLUTCH; // 狙击键，不常用
    let old_binding = mouse.binding(profile, target_button, cmd::layer::NORMAL).ok();
    let new_binding = Binding::key(cmd::modifier::CTRL, usage::A + 7); // Ctrl+H
    match mouse.set_binding(profile, target_button, cmd::layer::NORMAL, new_binding) {
        Ok(got) if got == new_binding => {
            let restored = match &old_binding {
                Some(old) => mouse
                    .set_binding(profile, target_button, cmd::layer::NORMAL, *old)
                    .map(|b| b == *old)
                    .unwrap_or(false),
                None => false,
            };
            report(
                "按键绑定",
                restored,
                format!("{} → Ctrl+H → 还原 {}", razer_core::button_label_zh(target_button),
                    if restored { "OK" } else { "失败" }),
            );
        }
        Ok(_) => {
            if let Some(old) = &old_binding {
                let _ = mouse.set_binding(profile, target_button, cmd::layer::NORMAL, *old);
            }
            report("按键绑定", false, "写入后读回不一致".into());
        }
        Err(e) => {
            if let Some(old) = &old_binding {
                let _ = mouse.set_binding(profile, target_button, cmd::layer::NORMAL, *old);
            }
            report("按键绑定", false, format!("{e}"));
        }
    }

    // ── DPI ──
    let old_dpi = mouse.dpi(profile)?;
    let test_dpi = if old_dpi.0 > 2000 { 1600 } else { 2400 };
    match mouse.set_dpi(profile, test_dpi, test_dpi) {
        Ok((x, y)) if x == test_dpi && y == test_dpi => {
            let back = mouse.set_dpi(profile, old_dpi.0, old_dpi.1);
            let restored = matches!(&back, Ok(v) if *v == old_dpi);
            report(
                "DPI",
                restored,
                format!("{} → {test_dpi} → 还原 {}",
                    old_dpi.0, if restored { "OK" } else { "失败" }),
            );
        }
        Ok((x, y)) => {
            let _ = mouse.set_dpi(profile, old_dpi.0, old_dpi.1);
            report("DPI", false, format!("写 {test_dpi} 读回 {x}×{y}"));
        }
        Err(e) => {
            let _ = mouse.set_dpi(profile, old_dpi.0, old_dpi.1);
            report("DPI", false, format!("{e}"));
        }
    }

    // ── 回报率 ──
    let old_hz = mouse.polling_hz(profile)?;
    let test_hz = if old_hz == 1000 { 500 } else { 1000 };
    match mouse.set_polling_hz(profile, test_hz) {
        Ok(got) if got == test_hz => {
            let back = mouse.set_polling_hz(profile, old_hz);
            let restored = matches!(&back, Ok(h) if *h == old_hz);
            report(
                "回报率",
                restored,
                format!("{old_hz} → {test_hz} → 还原 {}",
                    if restored { "OK" } else { "失败" }),
            );
        }
        Ok(got) => {
            let _ = mouse.set_polling_hz(profile, old_hz);
            report("回报率", false, format!("写 {test_hz} 读回 {got}"));
        }
        Err(e) => {
            let _ = mouse.set_polling_hz(profile, old_hz);
            report("回报率", false, format!("{e}"));
        }
    }

    // ── 只读项 ──
    let fw = mouse.firmware()?;
    report("固件读取", fw.0 > 0, format!("v{}.{}.{}", fw.0, fw.1, fw.2));
    let flash = mouse.flash()?;
    report(
        "Flash 读取",
        flash.total > 0,
        format!("{} B 可用 / {} B 总量", flash.immediately_usable(), flash.total),
    );
    let stages = mouse.dpi_stages(profile)?;
    report(
        "DPI 档位",
        !stages.stages.is_empty(),
        format!("{} 档，当前第 {} 档", stages.stages.len(), stages.active),
    );

    println!("\n结果：{pass} 通过，{fail} 失败");
    if fail > 0 {
        eprintln!("自检未全部通过");
    }
    Ok(())
}

fn describe_mode(mode: u8) -> String {
    let name = match mode {
        0 => "normal",
        1 => "bootloader",
        2 => "test",
        3 => "driver",
        _ => "unknown",
    };
    format!("{mode}:0 ({name})")
}

fn describe_binding(b: Binding) -> String {
    use cmd::function as f;
    match b.fn_id {
        x if x == f::OFF => "off".to_string(),
        x if x == f::MOUSE => mouse_action_name(b.data[0]).to_string(),
        x if x == f::MOUSE_ACTION_EXT => mouse_action_name(b.data[0]).to_string(),
        x if x == f::KEY => {
            let mods = b.data[0];
            let usage = b.data[1];
            let mut parts: Vec<String> = Vec::new();
            for (bit, name) in [
                (cmd::modifier::CTRL, "ctrl"),
                (cmd::modifier::SHIFT, "shift"),
                (cmd::modifier::ALT, "alt"),
                (cmd::modifier::WIN, "win"),
                (cmd::modifier::RCTRL, "ctrl"),
                (cmd::modifier::RSHIFT, "shift"),
                (cmd::modifier::RALT, "alt"),
                (cmd::modifier::RWIN, "win"),
            ] {
                if mods & bit != 0 {
                    parts.push(name.to_string());
                }
            }
            if usage != 0 {
                parts.push(key_name(usage));
            }
            format!("key:{}", parts.join("+"))
        }
        x if x == f::MACRO_TIMES => {
            let id = u16::from_be_bytes([b.data[0], b.data[1]]);
            let times = b.data[2];
            if times <= 1 {
                format!("macro:{id:04x}")
            } else {
                format!("macro:{id:04x}x{times}")
            }
        }
        x if x == f::MACRO_HELD => "macro:held".to_string(),
        x if x == f::MACRO_TOGGLE => "macro:toggle".to_string(),
        x if x == f::PROFILE => "profile:next".to_string(),
        x if x == f::WHEEL_MODE => "wheelmode".to_string(),
        x if x == f::DPI => match b.data[0] {
            cmd::dpi_action::UP => "dpi:up".to_string(),
            cmd::dpi_action::DOWN => "dpi:down".to_string(),
            cmd::dpi_action::CYCLE => "dpi:cycle".to_string(),
            other => format!("dpi:{other}"),
        },
        x if x == f::MEDIA => {
            let code = u16::from_be_bytes([b.data[0], b.data[1]]);
            format!("media:{}", media_name(code))
        }
        x if x == f::DOUBLE => "double".to_string(),
        x if x == f::HYPERSHIFT => "hypershift".to_string(),
        other => format!("fn=0x{other:02x} data={:02x?}", &b.data[..b.fn_size as usize]),
    }
}

fn mouse_action_name(code: u8) -> &'static str {
    use cmd::mouse_action as m;
    match code {
        m::LEFT => "left",
        m::RIGHT => "right",
        m::MIDDLE => "middle",
        m::BACK => "back",
        m::FORWARD => "forward",
        m::WHEEL_UP => "wheelup",
        m::WHEEL_DOWN => "wheeldown",
        m::TILT_LEFT => "tiltleft",
        m::TILT_RIGHT => "tiltright",
        _ => "mouse:?",
    }
}

fn media_name(code: u16) -> String {
    match code {
        0x00E9 => "volup".into(),
        0x00EA => "voldown".into(),
        0x00E2 => "mute".into(),
        0x00CD => "playpause".into(),
        0x00B5 => "next".into(),
        0x00B6 => "prev".into(),
        other => format!("0x{other:04x}"),
    }
}

/// 命令行里的按键名统一小写，方便肉眼比对和 grep。
fn key_name(u: u8) -> String {
    usage::label(u).to_lowercase()
}





fn parse_id(arg: Option<&String>) -> Option<u16> {
    arg.and_then(|s| parse_id_str(s))
}

fn parse_id_str(s: &str) -> Option<u16> {
    razer_core::parse_macro_id(s)
}

fn parse_speed(s: &str) -> Option<u8> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16).ok()
    } else {
        t.parse().ok()
    }
}

fn regions(zone: &str) -> Vec<u8> {
    match zone {
        "backlight" | "strip" => vec![reg::STRIP],
        "logo" => vec![reg::LOGO],
        "scroll" | "wheel" => vec![reg::WHEEL],
        "all" => vec![reg::WHEEL, reg::LOGO, reg::STRIP],
        _ => vec![reg::STRIP],
    }
}

fn parse_colour(spec: &str) -> Option<(u8, u8, u8)> {
    let s = spec.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some((r, g, b));
        }
    }
    if s.contains(',') {
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        if parts.len() == 3 {
            let r = parts[0].parse().ok()?;
            let g = parts[1].parse().ok()?;
            let b = parts[2].parse().ok()?;
            return Some((r, g, b));
        }
    }
    Some(match s.as_str() {
        "red" => (255, 0, 0),
        "green" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "white" => (255, 255, 255),
        "black" | "off" => (0, 0, 0),
        "cyan" => (0, 255, 255),
        "magenta" => (255, 0, 255),
        "yellow" => (255, 255, 0),
        "orange" => (255, 136, 0),
        "purple" => (128, 0, 255),
        "pink" => (255, 64, 160),
        "ice" => (0, 212, 255),
        "razer" => (68, 214, 44),
        _ => return None,
    })
}

fn print_flash(f: &FlashInfo) {
    println!("total      {} B ({} pages)", f.total, f.total / f.page_size());
    println!("used       {} B ({} pages)", f.used(), f.used() / f.page_size());
    println!(
        "available  {} B ({} pages, recycled included)",
        f.available,
        f.available / f.page_size()
    );
    println!(
        "recycled   {} B ({} pages, needs a format)",
        f.recycled,
        f.recycled / f.page_size()
    );
    println!(
        "usable     {} B ({} pages, right now)",
        f.immediately_usable(),
        f.immediately_usable() / f.page_size()
    );
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn print_help() {
    println!(
        r#"razerctl - Razer Basilisk V3 (1532:0099) control

  razerctl info                       device, mode, flash, macro count
  razerctl check                      every binding of the active slot
  razerctl flash                      flash pages: used / available / recycled
  razerctl mode                       read the device mode
  razerctl normal                     leave driver mode (fixes dead macros)
  razerctl paths                      HID interface paths, control port first

  razerctl slot                       the five slots at a glance
  razerctl slot show [N]              one slot, every region
  razerctl slot use <N>               switch slot, like the button does

  razerctl led <zone|all> <colour>    paint a zone one colour
  razerctl onboard <zone|all> static <colour> [--slot N]
  razerctl onboard <zone|all> spectrum [--slot N]
  razerctl onboard <zone|all> wave <dir 1|2> <speed> [--slot N]
  razerctl bright <0-100> [--slot N]  brightness, writes flash

  razerctl bind <button> <action>     action: key:ctrl+w, macro:0x0101, macro:0x0101x3
  razerctl macro list                 macros stored in flash
  razerctl macro show <id>            hex plus decoded
  razerctl macro add <id> <steps>     steps: "ctrl+shift+r, wait:120, text:ok"
  razerctl macro del <id>

Colours: a name (red, cyan, purple, razer, ...), #rrggbb, or 255,136,0.
Zones:   backlight (the strip), logo, scroll, all."#
    );
}
