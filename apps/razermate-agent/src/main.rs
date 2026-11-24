// RazerMate 后台守护进程
//
// 只做四件事，别的都不做：
//   1. 托盘图标，让用户知道它在跑，并且能退掉它；
//   2. 监听 HID 设备增减，鼠标拔插后自动重新同步；
//   3. 每 poll_ms 看一眼前台程序，按规则切板载槽；
//   4. 在命名管道上响应 GUI 的少量命令。
//
// 明确的取舍：
//   * 不常驻 HID 句柄。每次操作开一次关一次，鼠标不会被这个进程独占；
//   * 不用 async runtime。负载是「每 800 ms 一次 GetForegroundWindow + 必要时一次
//     HID 写入」，tokio 在这里是纯粹的内存和依赖负担；
//   * 硬件优先。检测到槽位是被别人（鼠标底部按键、CLI）改的，就退让若干秒，
//     不跟用户抢。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod log;
mod pipe;
mod window;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use razer_core::Mouse;
use razer_ipc::{client, paths, rules::Rules, vslots::{Index as VsIndex, SlotConfig}};

/// 最近一次由本进程写入的槽位。255 = 未知。
pub static LAST_APPLIED: AtomicU8 = AtomicU8::new(255);
/// 退让期截止时刻（毫秒，相对进程启动）。此期间不主动切槽。
pub static YIELD_UNTIL: AtomicU64 = AtomicU64::new(0);
/// 用户暂停了自动切换。
pub static PAUSED: AtomicBool = AtomicBool::new(false);
/// 进程启动时刻，作为所有相对时间的基准。
static START: AtomicU64 = AtomicU64::new(0);

/// 硬件改槽后，退让这么久再重新接管。
const YIELD_MS: u64 = 6000;

/// 最近一次由本进程套用的虚拟槽 ID。空串 = 当前不是虚拟槽在工作。
/// 用 Mutex 而不是 AtomicU8：ID 是字符串，且只在切槽时写，锁开销可忽略。
pub static LAST_VSLOT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn now_ms() -> u64 {
    let start = START.load(Ordering::Relaxed);
    if start == 0 {
        return 0;
    }
    unix_ms().saturating_sub(start)
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn main() -> std::process::ExitCode {
    START.store(unix_ms(), Ordering::Relaxed);

    match std::env::args().nth(1).as_deref() {
        // 命令行子命令：给测试和脚本用，不启动界面也不占托盘。
        Some("--status") => return cli_status(),
        Some("--slot") => return cli_slot(),
        Some("--quit") => return cli_quit(),
        Some("--pause") => return cli_pause(),
        Some("--rules") => return cli_rules(),
        Some("--reload") => return cli_reload(),
        Some("--fg") => return cli_fg(),
        Some("--vslots") => return cli_vslots(),
        Some("--vsapply") => return cli_vsapply(),
        Some("--help") | Some("-h") => {
            print_help();
            return std::process::ExitCode::SUCCESS;
        }
        _ => {}
    }

    run_agent()
}

fn print_help() {
    println!(
        "razermate-agent —— Basilisk V3 后台服务\n\
         \n\
         不带参数运行：托盘常驻，按规则自动切槽。\n\
         --status          查询运行状态\n\
         --slot N          手动切到第 N 号槽\n\
         --pause 0|1       暂停/恢复自动切换\n\
         --rules           打印当前规则\n\
         --reload          规则文件改完后让 agent 重新载入\n\
         --fg              打印 agent 认到的前台程序\n\
         --vslots          列出虚拟槽（在线模式）\n\
         --vsapply <id>    手动套用某个虚拟槽到 p0\n\
         --quit            让正在运行的实例退出"
    );
}

fn cli_status() -> std::process::ExitCode {
    match client::call("STATUS") {
        Ok(reply) => {
            println!("{reply}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn cli_slot() -> std::process::ExitCode {
    let Some(n) = std::env::args().nth(2).and_then(|s| s.parse::<u8>().ok()) else {
        eprintln!("用法：razermate-agent --slot <1-5>");
        return std::process::ExitCode::FAILURE;
    };
    match client::call(&format!("SLOT {n}")) {
        Ok(reply) => {
            println!("{reply}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn cli_pause() -> std::process::ExitCode {
    let Some(v) = std::env::args().nth(2) else {
        eprintln!("用法：razermate-agent --pause <0|1>");
        return std::process::ExitCode::FAILURE;
    };
    let on = if v == "1" || v.eq_ignore_ascii_case("true") { 1 } else { 0 };
    match client::call(&format!("PAUSE {on}")) {
        Ok(reply) => {
            println!("{reply}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn cli_rules() -> std::process::ExitCode {
    let rules = Rules::load_from(&paths::rules_path());
    print!("{}", rules.to_text());
    println!("# 文件位置：{}", paths::rules_path().display());
    std::process::ExitCode::SUCCESS
}

fn cli_reload() -> std::process::ExitCode {
    match client::call("RELOAD") {
        Ok(reply) => {
            println!("{reply}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn cli_fg() -> std::process::ExitCode {
    match client::call("FG") {
        Ok(reply) => {
            println!("{reply}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn cli_vslots() -> std::process::ExitCode {
    match client::call("VSLIST") {
        Ok(reply) => { println!("{reply}"); std::process::ExitCode::SUCCESS }
        Err(e) => { eprintln!("{e}"); std::process::ExitCode::FAILURE }
    }
}

fn cli_vsapply() -> std::process::ExitCode {
    let Some(id) = std::env::args().nth(2) else {
        eprintln!("用法：razermate-agent --vsapply <虚拟槽ID>");
        return std::process::ExitCode::FAILURE;
    };
    match client::call(&format!("VSAPPLY {id}")) {
        Ok(reply) => { println!("{reply}"); std::process::ExitCode::SUCCESS }
        Err(e) => { eprintln!("{e}"); std::process::ExitCode::FAILURE }
    }
}

fn cli_quit() -> std::process::ExitCode {    match client::call("QUIT") {
        Ok(reply) => {
            println!("{reply}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_agent() -> std::process::ExitCode {
    log::init();
    log::line("agent 启动");

    if let Err(e) = pipe::ensure_token() {
        log::line(&format!("生成令牌失败：{e}"));
    }

    // 单实例：已经有同名互斥体说明别人在跑，打个招呼就退出。
    match pipe::acquire_single_instance() {
        pipe::SingleInstance::AlreadyRunning => {
            log::line("已有实例在运行，唤醒它并退出");
            let _ = client::call("SHOW");
            return std::process::ExitCode::SUCCESS;
        }
        pipe::SingleInstance::Acquired => {}
    }

    pipe::spawn_server();

    let rules = Rules::load_from(&paths::rules_path());
    if !paths::rules_path().exists() {
        let _ = rules.save_to(&paths::rules_path());
    }
    log::line(&format!(
        "规则：{} 条，poll {} ms，default {}",
        rules.rules.len(),
        rules.poll_ms,
        rules.default_slot
    ));

    window::run_message_loop(rules)
}

/// 当前前台程序的 exe 名（小写）。拿不到就返回空串。
pub fn foreground_exe() -> String {
    window::foreground_process_name()
        .map(|p| razer_ipc::rules::exe_name_of(&p))
        .unwrap_or_default()
}

/// 读一次设备当前生效槽。失败返回 None（多半是鼠标拔了）。
pub fn device_active_slot() -> Option<u8> {
    Mouse::open().ok().and_then(|m| m.active_profile().ok())
}

/// 切槽并记账。返回给调用方的是一句人话。
pub fn apply_slot(slot: u8) -> Result<String, String> {
    let m = Mouse::open().map_err(|e| e.to_string())?;
    let got = m.set_active_profile(slot).map_err(|e| e.to_string())?;
    LAST_APPLIED.store(got, Ordering::Relaxed);
    // 切回板载槽意味着离开虚拟槽层。
    *LAST_VSLOT.lock().unwrap() = String::new();
    Ok(format!("已切换到 {got} 号槽"))
}

/// 按 ID 套用一个虚拟槽到 p0。
///
/// 内存关键：完整配置是这里的局部变量 `cfg`，套用完函数返回即释放；
/// 常驻内存的只有 `window.rs` 里的轻量索引（名字 + exe 规则）。
/// 这样即使有几百个虚拟槽，后台也只占用一个槽的解析开销。
pub fn apply_vslot(id: &str) -> Result<String, String> {
    let cfg = SlotConfig::load(id)
        .ok_or_else(|| format!("虚拟槽 {id} 的配置不存在"))?;
    let m = Mouse::open().map_err(|e| e.to_string())?;
    // 确保从 driver mode 回到 normal——虚拟槽靠固件执行宏，必须在 normal 下。
    m.ensure_normal().map_err(|e| e.to_string())?;
    let report = razer_core::apply_virtual_slot(&m, &cfg);
    *LAST_VSLOT.lock().unwrap() = id.to_string();
    let summary = report.summary_zh();
    log::line(&format!("虚拟槽 {id}: {summary}"));
    // 有失败项也要如实回报，但不算整体失败——部分配置已生效。
    Ok(summary)
}

/// 取得当前的虚拟槽索引（可能是 None，表示还没载入）。
pub fn vs_index() -> Option<VsIndex> {
    crate::window::vs_index()
}
