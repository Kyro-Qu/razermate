//! 命名管道的服务端：令牌、单实例、连接处理。
//!
//! 安全分两层，任何一层都不单独扛：
//!   1. 管道 DACL 只给 SYSTEM 和「所有者」——别的用户连不上；
//!   2. 连上之后还必须出示令牌，令牌文件在 `%LOCALAPPDATA%`，本身就只有本用户
//!      可读，用来挡同用户下的其他进程乱发命令。
//!
//! 另外管道带 `PIPE_REJECT_REMOTE_CLIENTS`，从别的机器连过来直接不通。

use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use razer_ipc::{paths, PIPE_NAME, OK_AUTHED};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::CreateMutexW;

use crate::{log, PAUSED, YIELD_UNTIL};

pub enum SingleInstance {
    Acquired,
    AlreadyRunning,
}

/// 建一个 `Local\` 命名互斥体。进程退出时系统自动释放，不会留下僵尸锁。
pub fn acquire_single_instance() -> SingleInstance {
    let name: Vec<u16> = "Local\\RazerMateAgent\0".encode_utf16().collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() {
            log::line("创建互斥体失败，按未运行处理");
            return SingleInstance::Acquired;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // 句柄留着不关也没关系：本进程马上就要退出了。
            SingleInstance::AlreadyRunning
        } else {
            SingleInstance::Acquired
        }
    }
}

/// 生成（或复用）共享令牌。
pub fn ensure_token() -> std::io::Result<()> {
    let path = paths::token_path();
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing.trim().len() == 64 {
            return Ok(());
        }
    }

    let mut buf = [0u8; 32];
    let ok = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if ok != 0 {
        // 退化成时间+PID 派生：强度差一些，但仍然是每次启动都不同。
        let seed = std::process::id() as u64
            ^ std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
        let mut x = seed | 1;
        for b in buf.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = (x & 0xFF) as u8;
        }
    }

    let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, hex.as_bytes())?;
    log::line("已生成新的管道令牌");
    Ok(())
}

fn read_token_file() -> String {
    std::fs::read_to_string(paths::token_path())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// 就绪后开一个线程循环接客。
pub fn spawn_server() {
    thread::spawn(|| {
        loop {
            if let Err(e) = serve_one() {
                log::line(&format!("管道服务异常：{e}"));
                thread::sleep(Duration::from_millis(400));
            }
        }
    });
}

fn serve_one() -> std::io::Result<()> {
    // 只给 SYSTEM 和所有者。OW = Owner Rights。
    let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;OW)\0".encode_utf16().collect();
    let mut sd: *mut c_void = std::ptr::null_mut();

    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1, // SDDL_REVISION_1
            &mut sd,
            std::ptr::null_mut(),
        ) == 0
        {
            log::line("构造安全描述符失败，改用默认 DACL");
            sd = std::ptr::null_mut();
        }
    }

    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };

    let name: Vec<u16> = format!("{PIPE_NAME}\0").encode_utf16().collect();
    let handle: HANDLE = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            // PIPE_ACCESS_DUPLEX
            0x0000_0003,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            if sd.is_null() { std::ptr::null_mut() } else { &mut sa },
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }

    // 阻塞等待客户端。
    unsafe {
        ConnectNamedPipe(handle, std::ptr::null_mut());
    }

    let result = handle_client(handle);

    unsafe {
        DisconnectNamedPipe(handle);
        CloseHandle(handle);
    }
    result
}

fn handle_client(handle: HANDLE) -> std::io::Result<()> {
    let expected = read_token_file();
    let mut authed = false;

    // 每连接独立状态，但要跟主线程共享 PAUSED / YIELD_UNTIL。
    loop {
        let Some(request) = read_line(handle)? else {
            return Ok(());
        };
        let request = request.trim();
        if request.is_empty() {
            continue;
        }

        let (cmd, arg) = match request.split_once(' ') {
            Some((c, a)) => (c, a.trim()),
            None => (request, ""),
        };

        if !authed {
            if cmd.eq_ignore_ascii_case("AUTH") && !expected.is_empty() && arg == expected {
                authed = true;
                write_line(handle, OK_AUTHED)?;
            } else {
                write_line(handle, "ERR auth")?;
                // 认证失败直接断开，不给爆破机会。
                return Ok(());
            }
            continue;
        }

        match cmd.to_ascii_uppercase().as_str() {
            "PING" => write_line(handle, "OK pong")?,
            // 给排查用的：agent 认为当前前台是哪个程序。规则不生效时先看这一条。
            "FG" => {
                let exe = crate::foreground_exe();
                let target = {
                    let g = razer_ipc::rules::Rules::load_from(&razer_ipc::paths::rules_path());
                    g.slot_for(&exe)
                };
                let vs_id = crate::vs_index().map(|i| i.slot_id_for(&exe)).unwrap_or_default();
                let shown = if exe.is_empty() { "-".to_string() } else { exe.clone() };
                write_line(handle, &format!(
                    "OK exe={shown} target={target} vslot={}",
                    if vs_id.is_empty() { "-".to_string() } else { vs_id }
                ))?;
            }
            "STATUS" => {
                // 只开一次设备：每次 open 都要枚举 HID，开两次纯属浪费。
                let active = crate::device_active_slot();
                let (slot, connected) = match active {
                    Some(s) => (s, 1u8),
                    None => (0, 0u8),
                };
                let rules = razer_ipc::rules::Rules::load_from(&razer_ipc::paths::rules_path());
                let idx = crate::vs_index().unwrap_or_default();
                let vs_cur = crate::LAST_VSLOT.lock().unwrap().clone();
                write_line(
                    handle,
                    &format!(
                        "OK slot={slot} connected={connected} paused={} rules={} poll={} vsenabled={} vscount={} vscur={}",
                        u8::from(PAUSED.load(Ordering::Relaxed)),
                        rules.rules.len(),
                        rules.poll_ms,
                        u8::from(idx.enabled),
                        idx.slots.len(),
                        if vs_cur.is_empty() { "-".to_string() } else { vs_cur }
                    ),
                )?;
            }
            "SLOT" => match arg.parse::<u8>() {
                Ok(n) if (1..=5).contains(&n) => match crate::apply_slot(n) {
                    Ok(msg) => write_line(handle, &format!("OK {msg}"))?,
                    Err(e) => write_line(handle, &format!("ERR {e}"))?,
                },
                _ => write_line(handle, "ERR slot 需要 1-5")?,
            },
            "PAUSE" => {
                let on = arg == "1";
                PAUSED.store(on, Ordering::Relaxed);
                // 恢复自动切换时，立刻让下一轮生效，不要卡在退让期里。
                if !on {
                    YIELD_UNTIL.store(0, Ordering::Relaxed);
                }
                log::line(if on { "自动切换已暂停" } else { "自动切换已恢复" });
                write_line(handle, if on { "OK paused" } else { "OK resumed" })?;
            }
            "RELOAD" => {
                let n = crate::window::reload_rules();
                let v = crate::window::reload_vslots();
                log::line(&format!("重新载入规则：{n} 条，虚拟槽 {v} 个"));
                write_line(handle, &format!("OK rules {n} vslots {v}"))?;
            }
            // ── 在线模式：虚拟槽 ──
            "VSLIST" => {
                // 返回索引里的槽清单：id|名字|exe 列表，逗号分隔槽。
                let idx = crate::vs_index().unwrap_or_default();
                let list: Vec<String> = idx.slots.iter().map(|s| {
                    format!("{}|{}|{}", s.id, s.name, s.exes.join(","))
                }).collect();
                write_line(handle, &format!(
                    "OK enabled={} default={} slots={}",
                    u8::from(idx.enabled), idx.default_id, list.join(";")
                ))?;
            }
            "VSAPPLY" => {
                // 手动套用某个虚拟槽，方便 GUI 预览 / 排障。
                let id = razer_ipc::vslots::safe_id(arg);
                match crate::apply_vslot(&id) {
                    Ok(msg) => write_line(handle, &format!("OK {msg}"))?,
                    Err(e) => write_line(handle, &format!("ERR {e}"))?,
                }
            }
            "VSRELOAD" => {
                let n = crate::window::reload_vslots();
                write_line(handle, &format!("OK vslots {n}"))?;
            }
            "VSSTATUS" => {
                let cur = crate::LAST_VSLOT.lock().unwrap().clone();
                let idx = crate::vs_index().unwrap_or_default();
                write_line(handle, &format!(
                    "OK enabled={} current={} count={}",
                    u8::from(idx.enabled), if cur.is_empty() { "-".to_string() } else { cur }, idx.slots.len()
                ))?;
            }
            "SHOW" => {
                crate::window::request_show_panel();
                write_line(handle, "OK shown")?;
            }
            "QUIT" => {
                write_line(handle, "OK bye")?;
                log::line("收到退出命令");
                // 让主线程走正常退出流程（会一并移除托盘图标）。
                crate::window::request_quit();
                return Ok(());
            }
            other => write_line(handle, &format!("ERR unknown {other}"))?,
        }
    }
}

/// 从管道读一行（读到 `\n` 为止）。对端关闭返回 `Ok(None)`。
fn read_line(handle: HANDLE) -> std::io::Result<Option<String>> {
    let mut out: Vec<u8> = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    loop {
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                handle,
                byte.as_mut_ptr(),
                1,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || read == 0 {
            if out.is_empty() {
                return Ok(None);
            }
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        if byte[0] != b'\r' {
            out.push(byte[0]);
            if out.len() > 512 {
                break;
            }
        }
    }
    Ok(Some(String::from_utf8_lossy(&out).into_owned()))
}

fn write_line(handle: HANDLE, s: &str) -> std::io::Result<()> {
    let mut buf = s.as_bytes().to_vec();
    buf.push(b'\n');
    let mut written = 0u32;
    let ok = unsafe {
        WriteFile(
            handle,
            buf.as_ptr(),
            buf.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
