//! 消息循环、托盘图标、热插拔、自动切槽。
//!
//! 用「普通但不可见的窗口」而不是 message-only 窗口：托盘菜单用
//! `TrackPopupMenu` 需要窗口能被 `SetForegroundWindow`，message-only 窗口做不到。
//!
//! 定时器固定 200 ms 心跳，实际做多少事由两个节流阀决定：
//!   * 前台程序检查：每 `poll_ms` 一次（默认 800 ms）；
//!   * 设备状态检查：每 4 s 一次，只为了发现「用户按了鼠标底部的 Profile 键」。
//! 这样待机时几乎不占 CPU，也不会每 800 ms 去做一次昂贵的 HID 枚举。

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use razer_ipc::rules::Rules;
use razer_ipc::vslots::Index as VsIndex;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    LoadIconW, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetTimer,
    TrackPopupMenu, TranslateMessage, MSG, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WNDCLASSW, WS_OVERLAPPED,
};

use crate::log;

const WM_APP: u32 = 0x8000;
const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_PANEL: u32 = WM_APP + 2;
const WM_RULES_CHANGED: u32 = WM_APP + 3;
const WM_TIMER_ID: usize = 1;
const TICK_MS: u32 = 200;
/// 设备状态巡检间隔。只为发现硬件改槽，不必太勤。
const DEVICE_CHECK_MS: u64 = 4000;

const TRAY_UID: u32 = 1;
const MENU_OPEN: usize = 1;
const MENU_PAUSE: usize = 2;
const MENU_QUIT: usize = 3;

static HWND_AGENT: AtomicIsize = AtomicIsize::new(0);
static RULES: Mutex<Option<Rules>> = Mutex::new(None);
/// 虚拟槽索引：每个槽只有 id / 名字 / exe 规则，轻量常驻。
/// 完整配置不缓存，按需从磁盘读、用完即弃——这是后台省内存的关键。
static VS_INDEX: Mutex<Option<VsIndex>> = Mutex::new(None);

/// 供 main.rs / pipe.rs 读取虚拟槽索引的快照（clone 出的索引很小）。
pub fn vs_index() -> Option<VsIndex> {
    VS_INDEX.lock().unwrap().clone()
}

/// 重新载入虚拟槽索引。GUI 改完配置后调它，无需重启 agent。
pub fn reload_vslots() -> usize {
    let idx = VsIndex::load();
    let n = idx.slots.len();
    *VS_INDEX.lock().unwrap() = Some(idx);
    // 规则变了，清空「同一程序不重复处理」的记忆，让下一拍重新判断。
    TICK.with(|t| t.borrow_mut().last_exe.clear());
    *crate::LAST_VSLOT.lock().unwrap() = String::new();
    n
}

thread_local! {
    static TICK: RefCell<Tick> = RefCell::new(Tick::default());
}

#[derive(Default)]
struct Tick {
    last_foreground_check: u64,
    last_device_check: u64,
    last_exe: String,
    tray_added: bool,
}

pub fn request_quit() {
    let h = HWND_AGENT.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            PostMessageW(h as HWND, 0x0010 /* WM_CLOSE */, 0, 0);
        }
    }
}

pub fn request_show_panel() {
    let h = HWND_AGENT.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            PostMessageW(h as HWND, WM_SHOW_PANEL, 0, 0);
        }
    }
}

/// 重新从磁盘读规则。GUI 改完规则文件后调它，不用重启守护进程。
pub fn reload_rules() -> usize {
    let rules = Rules::load_from(&razer_ipc::paths::rules_path());
    let n = rules.rules.len();
    *RULES.lock().unwrap() = Some(rules);
    // 规则变了，之前记的「同一个程序不重复处理」也要作废，否则要等用户切窗口。
    let h = HWND_AGENT.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            PostMessageW(h as HWND, WM_RULES_CHANGED, 0, 0);
        }
    }
    n
}

/// 当前前台进程的完整路径。
pub fn foreground_process_name() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if proc.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(proc, 0, buf.as_mut_ptr(), &mut len);
        windows_sys::Win32::Foundation::CloseHandle(proc);
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

pub fn run_message_loop(rules: Rules) -> std::process::ExitCode {
    *RULES.lock().unwrap() = Some(rules);
    // 虚拟槽索引同样在启动时载入一次，之后只按需读命中的那一个槽。
    *VS_INDEX.lock().unwrap() = Some(VsIndex::load());

    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class_name: Vec<u16> = "RazerMateAgentWnd\0".encode_utf16().collect();

        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            log::line("注册窗口类失败");
            return std::process::ExitCode::FAILURE;
        }

        let title: Vec<u16> = "RazerMate Agent\0".encode_utf16().collect();
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED, // 故意不带 WS_VISIBLE：窗口存在但不显示
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            log::line("创建窗口失败");
            return std::process::ExitCode::FAILURE;
        }
        HWND_AGENT.store(hwnd as isize, Ordering::Relaxed);

        add_tray(hwnd);
        SetTimer(hwnd, WM_TIMER_ID, TICK_MS, None);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    log::line("agent 退出");
    std::process::ExitCode::SUCCESS
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        0x0010 => {
            // WM_CLOSE：先摘托盘图标，再销毁窗口。
            remove_tray(hwnd);
            DestroyWindow(hwnd);
            0
        }
        0x0002 => {
            // WM_DESTROY
            PostQuitMessage(0);
            0
        }
        0x0113 => {
            // WM_TIMER
            on_tick();
            0
        }
        0x0219 => {
            // WM_DEVICECHANGE：设备树变了，之前记的槽位状态作废，下一轮重新同步。
            if wparam as u32 == 0x0007 {
                crate::LAST_APPLIED.store(255, Ordering::Relaxed);
                log::line("检测到设备变化，槽位状态作废");
            }
            0
        }
        WM_TRAY => {
            let event = (lparam as u32) & 0xFFFF;
            match event {
                0x0205 /* WM_RBUTTONUP */ | 0x007B /* WM_CONTEXTMENU */ => {
                    show_menu(hwnd);
                }
                0x0203 /* WM_LBUTTONDBLCLK */ => {
                    request_show_panel();
                }
                _ => {}
            }
            0
        }
        WM_SHOW_PANEL => {
            launch_panel();
            0
        }
        WM_RULES_CHANGED => {
            // 规则 / 虚拟槽配置换了，让下一拍重新判断前台程序。
            TICK.with(|t| t.borrow_mut().last_exe.clear());
            *VS_INDEX.lock().unwrap() = Some(VsIndex::load());
            *crate::LAST_VSLOT.lock().unwrap() = String::new();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn add_tray(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = LoadIconW(std::ptr::null_mut(), 32512 as *const u16); // IDI_APPLICATION
    let tip: Vec<u16> = "RazerMate 后台服务\0".encode_utf16().collect();
    let n = tip.len().min(nid.szTip.len());
    nid.szTip[..n].copy_from_slice(&tip[..n]);

    if Shell_NotifyIconW(NIM_ADD, &nid) == 0 {
        log::line("添加托盘图标失败");
    } else {
        TICK.with(|t| t.borrow_mut().tray_added = true);
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

unsafe fn remove_tray(hwnd: HWND) {
    // 没加上就别删，否则资源管理器会因为不存在的图标弹错误提示。
    if !TICK.with(|t| t.borrow().tray_added) {
        return;
    }
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    Shell_NotifyIconW(NIM_DELETE, &nid);
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let open: Vec<u16> = "打开控制面板\0".encode_utf16().collect();
    let pause_label: Vec<u16> = if crate::PAUSED.load(Ordering::Relaxed) {
        "恢复自动切换\0".encode_utf16().collect()
    } else {
        "暂停自动切换\0".encode_utf16().collect()
    };
    let quit: Vec<u16> = "退出\0".encode_utf16().collect();

    AppendMenuW(menu, 0x0000, MENU_OPEN, open.as_ptr());
    AppendMenuW(menu, 0x0000, MENU_PAUSE, pause_label.as_ptr());
    AppendMenuW(menu, 0x0800 /* MF_SEPARATOR */, 0, std::ptr::null());
    AppendMenuW(menu, 0x0000, MENU_QUIT, quit.as_ptr());

    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);

    // 这一对调用是托盘菜单的标准套路：不先把自己设成前台，点别处菜单不会消失。
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD,
        pt.x,
        pt.y,
        0,
        hwnd,
        std::ptr::null(),
    );
    PostMessageW(hwnd, 0x0000 /* WM_NULL */, 0, 0);
    DestroyMenu(menu);

    match cmd as usize {
        MENU_OPEN => launch_panel(),
        MENU_PAUSE => {
            let now = !crate::PAUSED.load(Ordering::Relaxed);
            crate::PAUSED.store(now, Ordering::Relaxed);
            if !now {
                crate::YIELD_UNTIL.store(0, Ordering::Relaxed);
            }
            log::line(if now { "托盘：暂停自动切换" } else { "托盘：恢复自动切换" });
        }
        MENU_QUIT => {
            log::line("托盘：退出");
            PostMessageW(hwnd, 0x0010 /* WM_CLOSE */, 0, 0);
        }
        _ => {}
    }
}

/// 拉起面板程序（和 agent 同目录的 razermate.exe）。
fn launch_panel() {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("razermate.exe")));
    let Some(exe) = exe else { return };
    if !exe.exists() {
        log::line(&format!("找不到面板程序：{}", exe.display()));
        return;
    }
    match std::process::Command::new(&exe).spawn() {
        Ok(_) => log::line("已拉起控制面板"),
        Err(e) => log::line(&format!("拉起面板失败：{e}")),
    }
}

fn on_tick() {
    let now = crate::now_ms();

    // 200 ms 一拍，规则每拍都拷一份太浪费。这里只取两个标量。
    let (enabled, poll_ms) = {
        let guard = RULES.lock().unwrap();
        match guard.as_ref() {
            Some(r) => (r.enabled, r.poll_ms as u64),
            None => return,
        }
    };

    // ── 1. 设备巡检：看槽位是不是被别人改过 ──
    let mut need_device_check = false;
    TICK.with(|t| {
        let mut t = t.borrow_mut();
        if now.saturating_sub(t.last_device_check) >= DEVICE_CHECK_MS {
            t.last_device_check = now;
            need_device_check = true;
        }
    });

    if need_device_check {
        match crate::device_active_slot() {
            Some(active) => {
                let last = crate::LAST_APPLIED.load(Ordering::Relaxed);
                if last == 255 {
                    // 首次同步：只记账，不动设备，避免刚启动就把用户切走。
                    crate::LAST_APPLIED.store(active, Ordering::Relaxed);
                    log::line(&format!("首次同步：当前 {active} 号槽"));
                } else if active != last {
                    // 不是我们改的：用户按了底部按键，或者别的程序改的。退让。
                    crate::LAST_APPLIED.store(active, Ordering::Relaxed);
                    let until = now + crate::YIELD_MS;
                    crate::YIELD_UNTIL.store(until, Ordering::Relaxed);
                    log::line(&format!(
                        "槽位被外部改为 {active}，退让 {} 秒",
                        crate::YIELD_MS / 1000
                    ));
                }
            }
            None => {
                // 鼠标不在。清掉状态，插回来时重新同步。
                crate::LAST_APPLIED.store(255, Ordering::Relaxed);
            }
        }
    }

    if !enabled || crate::PAUSED.load(Ordering::Relaxed) {
        return;
    }
    if now < crate::YIELD_UNTIL.load(Ordering::Relaxed) {
        return;
    }

    // ── 2. 前台程序检查（节流到 poll_ms） ──
    let mut do_foreground = false;
    TICK.with(|t| {
        let mut t = t.borrow_mut();
        if now.saturating_sub(t.last_foreground_check) >= poll_ms {
            t.last_foreground_check = now;
            do_foreground = true;
        }
    });
    if !do_foreground {
        return;
    }

    let exe = crate::foreground_exe();
    if exe.is_empty() {
        return;
    }

    // 同一个程序不重复处理，省掉每次都要读设备。
    let changed = TICK.with(|t| {
        let mut t = t.borrow_mut();
        if t.last_exe == exe {
            false
        } else {
            t.last_exe = exe.clone();
            true
        }
    });
    if !changed {
        return;
    }

    // ── 3. 在线模式优先：按前台 exe 查虚拟槽索引 ──
    // 索引常驻但很小；只有命中时才读那一个槽的完整配置并套用到 p0。
    {
        let vs_enabled = VS_INDEX.lock().unwrap().as_ref().map(|i| i.enabled).unwrap_or(false);
        if vs_enabled {
            let vs_id = VS_INDEX.lock().unwrap().as_ref().map(|i| i.slot_id_for(&exe)).unwrap_or_default();
            let current_vs = crate::LAST_VSLOT.lock().unwrap().clone();
            if !vs_id.is_empty() && vs_id != current_vs {
                match crate::apply_vslot(&vs_id) {
                    Ok(msg) => log::line(&format!("{exe} → {msg}")),
                    Err(e) => log::line(&format!("{exe} → 虚拟槽失败：{e}")),
                }
                return;
            }
            // 已在该虚拟槽上，或索引明确匹配到同一个：不再动设备。
            if !vs_id.is_empty() {
                return;
            }
            // 该程序没有虚拟槽规则（且无默认槽）：落回下面的板载槽逻辑。
        }
    }

    // ── 4. 离线模式：按板载槽规则切换 ──
    let target = {
        let guard = RULES.lock().unwrap();
        guard.as_ref().map(|r| r.slot_for(&exe)).unwrap_or(0)
    };
    if target == 0 {
        return;
    }

    let current = crate::LAST_APPLIED.load(Ordering::Relaxed);
    if current == target {
        return;
    }

    match crate::apply_slot(target) {
        Ok(msg) => log::line(&format!("{exe} → {msg}")),
        Err(e) => log::line(&format!("{exe} → 切槽失败：{e}")),
    }
}
