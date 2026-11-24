//! 极简日志：追加写 `%LOCALAPPDATA%\RazerMate\agent.log`。
//!
//! 守护进程没有控制台，出问题时唯一能看的就是这个文件。不引入日志框架——
//! 这里只需要「一行、带时间、别丢」。

use std::io::Write;
use std::sync::Mutex;

use razer_ipc::paths;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

static LOCK: Mutex<()> = Mutex::new(());

pub fn init() {
    // 超过 256 KB 就轮转一次，避免无限增长。
    let path = paths::log_path();
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 256 * 1024 {
            let _ = std::fs::rename(&path, path.with_extension("old.log"));
        }
    }
}

pub fn line(msg: &str) {
    let _guard = LOCK.lock();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::log_path())
    {
        let _ = writeln!(f, "{}  {msg}", stamp());
    }
}

/// `HH:MM:SS`，取自本地时间，不用为一个格式化数字引日期库。
fn stamp() -> String {
    unsafe {
        let mut st = std::mem::zeroed();
        GetLocalTime(&mut st);
        format!(
            "{:02}:{:02}:{:02}",
            st.wHour, st.wMinute, st.wSecond
        )
    }
}
