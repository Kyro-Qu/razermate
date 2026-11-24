//! RazerMate v2 后台程序：无 GUI、无托盘、无窗口消息循环。
//!
//! 它是一台**通用解释器**：不认识任何按键，不内置「哪些键固定」，不判断
//! 哪个应用该用什么颜色。按钮 ID 表、动作串、覆盖规则、灯区、匹配条件、
//! 轮询间隔全部来自 `profiles.json`。改配置 = 改行为，永远不必重编译。
//!
//! 唯一硬件动作：把合并后的配置差量写进 p0（direct profile / RAM 工作层）。
//! 绝不读写、切换或擦除 5 个板载配置槽。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use razer_config::{Config, LogPolicy};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, SYSTEMTIME,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "RazerMateHeadless";

// ────────────────────────────── 日志 ──────────────────────────────

struct LogState {
    enabled: bool,
    path: PathBuf,
    max_kb: u32,
    echo: bool,
}

static LOG: OnceLock<Mutex<LogState>> = OnceLock::new();

fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("RazerMate")
}

fn init_log(echo: bool) {
    LOG.get_or_init(|| {
        Mutex::new(LogState {
            enabled: true,
            path: log_dir().join("headless.log"),
            max_kb: 512,
            echo,
        })
    });
    let _ = std::fs::create_dir_all(log_dir());
}

fn reconfigure_log(p: &LogPolicy) {
    let Some(m) = LOG.get() else { return };
    let mut st = m.lock().unwrap();
    st.enabled = p.enabled;
    st.max_kb = p.max_kb.max(1);
    if !p.path.is_empty() {
        st.path = PathBuf::from(&p.path);
    }
    if let Some(parent) = st.path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

fn hhmmss() -> String {
    unsafe {
        let mut t: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut t);
        format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
    }
}

fn log(msg: &str) {
    let Some(m) = LOG.get() else { return };
    let st = m.lock().unwrap();
    let line = format!("{}  {}", hhmmss(), msg);
    if st.echo {
        println!("{line}");
    }
    if !st.enabled {
        return;
    }
    // 简单轮转：超过上限就只留后半段，避免后台常年运行把日志撑大。
    if let Ok(meta) = std::fs::metadata(&st.path) {
        let limit = st.max_kb as u64 * 1024;
        if meta.len() > limit {
            if let Ok(old) = std::fs::read(&st.path) {
                let keep = (limit / 2) as usize;
                let start = old.len().saturating_sub(keep);
                let _ = std::fs::write(&st.path, &old[start..]);
            }
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&st.path)
    {
        let _ = writeln!(f, "{line}");
    }
}

// ────────────────────────────── Win32 小工具 ──────────────────────────────

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// CLI 模式下把 stdout 接回父控制台（从资源管理器双击时会失败，忽略即可）。
fn attach_console() {
    unsafe {
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        windows_sys::Win32::System::Console::AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn acquire_single_instance() -> bool {
    let name: Vec<u16> = "Local\\RazerMateHeadless\0".encode_utf16().collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() {
            return true;
        }
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// (前台 exe 小写文件名, 窗口标题, 进程完整路径)
fn foreground_info() -> (String, String, String) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return Default::default();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut tbuf = [0u16; 512];
        let n = GetWindowTextW(hwnd, tbuf.as_mut_ptr(), tbuf.len() as i32);
        let title = if n > 0 {
            String::from_utf16_lossy(&tbuf[..n as usize])
        } else {
            String::new()
        };
        let mut full = String::new();
        if pid != 0 {
            let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !proc.is_null() {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(proc, 0, buf.as_mut_ptr(), &mut len) != 0 {
                    full = String::from_utf16_lossy(&buf[..len as usize]);
                }
                CloseHandle(proc);
            }
        }
        let exe = Path::new(&full)
            .file_name()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        (exe, title, full)
    }
}

fn autostart_query() -> bool {
    unsafe {
        let sub = wide(RUN_SUBKEY);
        let name = wide(RUN_VALUE);
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return false;
        }
        let mut kind = 0u32;
        let mut size = 0u32;
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

fn autostart_set(command: Option<&str>) -> Result<(), String> {
    unsafe {
        let sub = wide(RUN_SUBKEY);
        let name = wide(RUN_VALUE);
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_READ | KEY_WRITE,
            &mut key,
        ) != 0
        {
            return Err("打不开 HKCU 启动项".into());
        }
        let status = match command {
            Some(value) => {
                let v = wide(value);
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    v.as_ptr().cast::<u8>(),
                    (v.len() * 2) as u32,
                )
            }
            None => RegDeleteValueW(key, name.as_ptr()),
        };
        RegCloseKey(key);
        if status != 0 {
            return Err(format!("写启动项失败，Win32 错误 {status}"));
        }
        Ok(())
    }
}

// ────────────────────────────── 配置与下发 ──────────────────────────────

fn load_config(path: &Path) -> Result<Config, String> {
    razer_config::ensure_default(path)?;
    let cfg = Config::load(path)?;
    cfg.validate().map_err(|e| e.join("；"))?;
    for w in cfg.warnings() {
        log(&format!("配置提示：{w}"));
    }
    Ok(cfg)
}

fn file_stamp(p: &Path) -> (u128, u64) {
    match std::fs::metadata(p) {
        Ok(m) => {
            let t = m.modified().ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or(0);
            (t, m.len())
        }
        Err(_) => (0, 0),
    }
}

/// 把某一组配置编译后写进 p0。设备层自己做读回比对与差量写。
fn apply(cfg: &Config, id: &str) -> Result<String, String> {
    let slot = cfg.compile(id)?;
    let mouse = razer_core::Mouse::open().map_err(|e| format!("设备未连接：{e}"))?;
    if cfg.service.p0.normal_mode_only {
        mouse.ensure_normal().map_err(|e| format!("恢复 normal 模式失败：{e}"))?;
    }
    let report = razer_core::apply_virtual_slot(&mouse, &slot);
    if !report.errors.is_empty() {
        log(&format!("下发有失败项：{}", report.errors.join("；")));
    }
    Ok(report.summary_zh())
}

fn run_once(path: &Path, forced: Option<String>) -> std::process::ExitCode {
    let cfg = match load_config(path) {
        Ok(c) => c,
        Err(e) => {
            log(&format!("配置无效：{e}"));
            return std::process::ExitCode::FAILURE;
        }
    };
    reconfigure_log(&cfg.service.log);
    let (exe, title, full) = foreground_info();
    let id = forced.unwrap_or_else(|| {
        cfg.select_profile(&exe, &title, &full)
            .unwrap_or(cfg.service.fallback.as_str())
            .to_string()
    });
    match apply(&cfg, &id) {
        Ok(msg) => {
            log(&format!("一次性下发：组={id} 前台={exe} | {msg}"));
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            log(&format!("一次性下发失败：{e}"));
            std::process::ExitCode::FAILURE
        }
    }
}

// ────────────────────────────── 常驻主循环 ──────────────────────────────

fn run_daemon(path: &Path) -> std::process::ExitCode {
    if !acquire_single_instance() {
        log("已有实例在运行，本进程退出");
        return std::process::ExitCode::SUCCESS;
    }
    let mut cfg = match load_config(path) {
        Ok(c) => c,
        Err(e) => {
            log(&format!("启动失败，配置无效：{e}"));
            return std::process::ExitCode::FAILURE;
        }
    };
    reconfigure_log(&cfg.service.log);
    log(&format!(
        "headless 启动 | 配置 {} | {} 组 | 轮询 {} ms | 永不触碰板载槽",
        path.display(), cfg.profiles.len(), cfg.service.poll_ms
    ));

    let mut stamp = file_stamp(path);
    let mut last_profile = String::new();
    let mut last_config_check = Instant::now() - Duration::from_secs(60);
    let mut last_poll = Instant::now() - Duration::from_secs(60);
    let mut last_apply = Instant::now();
    let mut pending_change: Option<Instant> = None;
    let mut device_missing_logged = false;

    loop {
        let now = Instant::now();

        // 1) 配置热更新：mtime + 长度，去抖后重载；坏文件保留上一份。
        if now.duration_since(last_config_check)
            >= Duration::from_millis(cfg.service.config_watch_ms)
        {
            last_config_check = now;
            let s = file_stamp(path);
            if s != stamp && pending_change.is_none() {
                pending_change = Some(now);
            }
        }
        if let Some(t) = pending_change {
            if now.duration_since(t) >= Duration::from_millis(cfg.service.config_debounce_ms) {
                pending_change = None;
                match load_config(path) {
                    Ok(new) => {
                        cfg = new;
                        stamp = file_stamp(path);
                        last_profile.clear(); // 强制用新配置重下发一次
                        reconfigure_log(&cfg.service.log);
                        log("配置已热更新，重新下发");
                    }
                    Err(e) => {
                        // keep_last：绝不把半截配置写进设备。
                        log(&format!("配置写坏，保留上一份有效配置：{e}"));
                    }
                }
            }
        }

        // 2) 前台应用匹配 + 定时重下发（应对切槽 / 唤醒覆盖 p0）。
        let due_poll = now.duration_since(last_poll) >= Duration::from_millis(cfg.service.poll_ms);
        let due_reapply = cfg.service.reapply_interval_ms > 0
            && now.duration_since(last_apply)
                >= Duration::from_millis(cfg.service.reapply_interval_ms);
        if due_poll || due_reapply {
            last_poll = now;
            let (exe, title, full) = foreground_info();
            let id = cfg.select_profile(&exe, &title, &full)
                .unwrap_or(cfg.service.fallback.as_str())
                .to_string();
            if id != last_profile || due_reapply {
                match apply(&cfg, &id) {
                    Ok(msg) => {
                        log(&format!("组={id} 前台={exe} | {msg}"));
                        device_missing_logged = false;
                    }
                    Err(e) => {
                        if !device_missing_logged {
                            log(&format!("下发失败（静默重试中）：{e}"));
                            device_missing_logged = true;
                        }
                        // 没写成功就不记账，下一拍继续尝试。
                        last_apply = now;
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                }
                last_profile = id;
                last_apply = now;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ────────────────────────────── CLI ──────────────────────────────

fn help() {
    println!(
        "razermate-headless —— v2 无 GUI 后台程序（纯 JSON 驱动，只写 p0）\n\
         \n\
         不带参数 / --run        后台常驻：无窗口、无托盘、开机自启走的就是这个\n\
         --once                 只下发一次当前前台对应的配置，然后退出\n\
         --apply <组id>         强制套用某一组后退出\n\
         --validate             校验配置（含全部动作串、宏引用、灯区）\n\
         --status               显示配置路径、当前组、自启状态\n\
         --init                 在配置路径生成默认 profiles.json\n\
         --install              写 HKCU 开机自启（会带上 --config 参数）\n\
         --uninstall            删除开机自启\n\
         --config <路径>        指定 JSON 路径；默认 %LOCALAPPDATA%\\RazerMate\\profiles.json\n\
         --debug                常驻时同时把日志打到控制台\n\
         --help                 本帮助\n\
         \n\
         改 JSON 约 1 秒内生效；写坏了保留上一份有效配置，绝不写设备。"
    );
}

fn main() -> std::process::ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut cmd: Option<String> = None;
    let mut config_override: Option<PathBuf> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut debug = false;
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--config" => {
                if let Some(p) = raw.get(i + 1) {
                    config_override = Some(PathBuf::from(p));
                }
                i += 2;
            }
            "--debug" => { debug = true; i += 1; }
            s if s.starts_with("--") && cmd.is_none() => { cmd = Some(s.to_string()); i += 1; }
            s => { rest.push(s.to_string()); i += 1; }
        }
    }
    let command = cmd.as_deref();
    let is_cli = !matches!(command, None | Some("--run"));
    if is_cli {
        attach_console();
    }
    init_log(debug || is_cli);

    let path = config_override.clone().unwrap_or_else(razer_config::runtime_path);

    match command {
        Some("--help") | Some("-h") => { help(); std::process::ExitCode::SUCCESS }
        Some("--init") => {
            match razer_config::ensure_default(&path) {
                Ok(true) => { log(&format!("已生成默认配置：{}", path.display())); std::process::ExitCode::SUCCESS }
                Ok(false) => { log(&format!("配置已存在，未覆盖：{}", path.display())); std::process::ExitCode::SUCCESS }
                Err(e) => { log(&format!("生成失败：{e}")); std::process::ExitCode::FAILURE }
            }
        }
        Some("--validate") => {
            let p = rest.first().map(PathBuf::from).unwrap_or(path);
            match load_config(&p) {
                Ok(c) => {
                    log(&format!(
                        "配置有效 | schema {} | 按钮 {} | 配置组 {} | 宏 {} | 路径 {}",
                        c.schema, c.device.buttons.len(), c.profiles.len(), c.macros.len(), p.display()
                    ));
                    std::process::ExitCode::SUCCESS
                }
                Err(e) => { log(&format!("配置无效：{e}")); std::process::ExitCode::FAILURE }
            }
        }
        Some("--status") => {
            let exists = path.exists();
            let stamp = file_stamp(&path);
            let (exe, title, full) = foreground_info();
            if exists {
                if let Ok(c) = load_config(&path) {
                    let chosen = c.select_profile(&exe, &title, &full)
                        .unwrap_or(c.service.fallback.as_str());
                    log(&format!(
                        "状态 | 配置 {} | schema {} | {} 组 | 前台 {} | 选中组 {} | 自启 {} | mtime {} len {}",
                        path.display(), c.schema, c.profiles.len(), exe, chosen,
                        if autostart_query() { "开" } else { "关" }, stamp.0, stamp.1
                    ));
                } else {
                    log(&format!("状态 | 配置 {} 存在但无效 | 自启 {}", path.display(),
                        if autostart_query() { "开" } else { "关" }));
                }
            } else {
                log(&format!("状态 | 配置 {} 不存在（启动会自动生成）", path.display()));
            }
            std::process::ExitCode::SUCCESS
        }
        Some("--install") => {
            let exe = match std::env::current_exe() {
                Ok(p) => p,
                Err(e) => { log(&format!("取不到自身路径：{e}")); return std::process::ExitCode::FAILURE; }
            };
            let mut value = format!("\"{}\" --run", exe.display());
            if let Some(p) = &config_override {
                value.push_str(&format!(" --config \"{}\"", p.display()));
            }
            match autostart_set(Some(&value)) {
                Ok(()) => { log(&format!("已写入开机自启：{value}")); std::process::ExitCode::SUCCESS }
                Err(e) => { log(&format!("写自启失败：{e}")); std::process::ExitCode::FAILURE }
            }
        }
        Some("--uninstall") => match autostart_set(None) {
            Ok(()) => { log("已删除开机自启"); std::process::ExitCode::SUCCESS }
            Err(e) => { log(&format!("删自启失败：{e}")); std::process::ExitCode::FAILURE }
        },
        Some("--once") => run_once(&path, None),
        Some("--apply") => match rest.first() {
            Some(id) => run_once(&path, Some(id.clone())),
            None => { log("用法：--apply <配置组 id>"); std::process::ExitCode::FAILURE }
        },
        Some("--run") | None => run_daemon(&path),
        Some(other) => { log(&format!("未知命令 {other}；--help 看用法")); std::process::ExitCode::FAILURE }
    }
}
