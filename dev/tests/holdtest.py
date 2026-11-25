#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Basilisk V3 宏 hold / toggle 触发方式的人工按键验收（可复现）。

设备上 `fn=4`(按住) / `fn=5`(开关) 的绑定能写入、能读回，但按下去是否真的按
预期播放，只有物理按键能回答。这个脚本把「播放了几次」变成可数的字符：

    h = 按住触发（dpi 键，按住期间应连续出现）
    t = 开关触发（forward 键，点一次后应自己连续出现，再点一次停）
    c = 对照组（back 键，普通单次宏，按一次只出 1 个 c）

三个宏每次只吐一个字符 + wait:300，所以 2 秒大约 6 轮，计数直接可读。
绑定全部写在 p0（当前生效配置，不碰 5 个板载槽），宏本体写 Flash，收尾时删除。

用法：
    python tests/holdtest.py --prepare   只写宏 + 绑定 + 读回校验（分步调试用）
    python tests/holdtest.py --watch     设置 + 置顶观察窗 + 用户按键 + 自动还原
    python tests/holdtest.py --restore   幂等强制还原，任何时候都能单独调用
    python tests/holdtest.py --selftest  离线验证判读逻辑，不碰设备

结果全部走 stdout，不产生临时文件。收尾是硬保证：点按钮、自动判定或超时都会
执行 restore；即便进程被外部杀掉，也可以手动跑一次 --restore 清场。
"""
from __future__ import annotations

import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CTL = ROOT / "versions" / "v1-gui" / "dist" / "RazerMate" / "razerctl.exe"
CREATE_NO_WINDOW = 0x08000000
TIMEOUT_S = 180
IDLE_FINISH_S = 15

# id -> 宏步骤文本
MACROS: dict[int, str] = {
    0x0301: "text:h, wait:300",  # hold
    0x0302: "text:t, wait:300",  # toggle
    0x0303: "text:c, wait:300",  # 对照组：普通单次宏
}

# (按钮, 动作串)
BINDS: list[tuple[str, str]] = [
    ("dpi", "macrohold:0x0301"),
    ("forward", "macrotoggle:0x0302"),
    ("back", "macro:0x0303"),
]

# Win32 Static 控件对多行 \n 的排版不可靠（会叠行），所以一行一个控件。
INSTRUCTION_LINES = [
    "【先点一下下面的白色文本框，让光标停在里面】",
    "保持本窗口在前台；不要点其它窗口，也不要按鼠标底部的 Profile 键。",
    "",
    "第 1 步  轻点「后退」侧键 → 应立刻出现 1 个 c（对照组）",
    "第 2 步  按住滚轮后方的 DPI 键约 2 秒不松 → 看 h 是否连续冒出；松开应立刻停止",
    "第 3 步  轻点「前进」侧键 → 等 2 秒看 t 是否自己连续冒出；再点一次应停止",
    "第 4 步  测完点【结束并还原】；或 180 秒后自动收尾",
    "",
    "脚本会自动停止宏、删除 3 个测试宏、切槽还原 p0 并逐项校验。",
    "若 t 停不下来，直接点【结束并还原】即可强制停止。",
]

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")


def ctl(*args: str) -> tuple[int, str]:
    """跑一条 razerctl 命令，把输出同步打到 stdout（不弹控制台窗口）。"""
    print(f"$ razerctl {' '.join(args)}", flush=True)
    try:
        p = subprocess.run(
            [str(CTL), *args],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            creationflags=CREATE_NO_WINDOW,
            timeout=30,
        )
        out = ((p.stdout or "") + (p.stderr or "")).strip()
        code = p.returncode
    except Exception as e:  # 设备忙 / 被占用，也要继续走完还原流程
        out, code = f"命令异常: {e}", -1
    for line in out.splitlines():
        print("   " + line, flush=True)
    return code, out


def baseline() -> dict[str, str]:
    """记录还原后要对照的 p0 原始绑定。"""
    print("=== 基线 ===", flush=True)
    ctl("info")
    snap: dict[str, str] = {}
    for btn, _ in BINDS:
        _, out = ctl("binding", btn, "--slot", "0")
        snap[btn] = out.strip()
    return snap


def prepare() -> None:
    print("=== 写入 3 个可计数宏 ===", flush=True)
    for mid, spec in MACROS.items():
        ctl("macro", "add", f"0x{mid:04x}", spec)
    print("=== 写入 p0 的 3 个绑定 ===", flush=True)
    for btn, action in BINDS:
        ctl("bind", btn, action, "--slot", "0")
    print("=== 读回校验（fn 应为 4 / 5 / 3）===", flush=True)
    for btn, _ in BINDS:
        ctl("binding", btn, "--slot", "0")


def restore(snap: dict[str, str] | None = None) -> str:
    """幂等强制还原：立刻停触发 → 删宏 → 切槽覆盖 p0 → 逐项校验。"""
    print("=== 收尾：强制还原 ===", flush=True)
    # 1) 先把三个键置 off，立刻掐断可能仍在循环的 toggle 宏
    for btn, _ in BINDS:
        ctl("bind", btn, "off", "--slot", "0")
    time.sleep(0.6)
    # 2) 删除测试宏（macro del 会同步注销登记表）
    for mid in MACROS:
        ctl("macro", "del", f"0x{mid:04x}")
    # 3) 切槽，让硬件把板载槽 4 的配置重新灌进 p0
    ctl("slot", "use", "3")
    ctl("slot", "use", "4")
    # 4) 校验
    print("--- 还原后校验 ---", flush=True)
    ctl("info")
    ctl("macro", "list")
    ok = True
    for btn, _ in BINDS:
        _, out = ctl("binding", btn, "--slot", "0")
        if snap and btn in snap and snap[btn] and out.strip() != snap[btn]:
            print(f"   [注意] {btn} 的 p0 绑定与基线不同：基线={snap[btn]!r}", flush=True)
            ok = False
    return "OK" if ok else "有差异（见上）"


def verdict(capture: str, idle_s: float | None = None) -> str:
    """把采集到的字符判读成结论。

    hold 的「松开后停止」是可以自动判定的：如果按住期间启动的循环没被松开
    掐断，后面整段 toggle 输出里就会继续夹杂 h。真实样本是
    'chhhhhhttt...' —— h 全在 t 之前，之后 0 个 h，即松开生效。
    idle_s 是收尾前最后一次字符变化到收尾的间隔，用来判断 toggle 是否被
    第二次点击停下；若脚本是强制收尾，则不能据此声称已人工停下。
    """
    h, t, c = capture.count("h"), capture.count("t"), capture.count("c")
    lines = [f"采集字符：h={h}  t={t}  c={c}", f"原始文本：{capture!r}"]
    if c == 0:
        lines.append("对照组 c=0 → 宏本体/输入链路没通，本次按键结果不可判读，需重做。")
        return "\n".join(lines)
    lines.append("对照组 c>=1 → 宏本体、p0 绑定、键盘输入链路均正常。")
    if h == 0:
        lines.append("hold：0 次 → 未捕获到 h。若确实按住了 DPI 键，说明 fn=4 物理上不触发宏；"
                     "也可能是本轮根本没按该键，需结合人工确认。")
    elif h == 1:
        lines.append("hold：1 次 → 只播放一次，不是「按住期间循环」。")
    else:
        stopped = capture.rfind("h") < capture.find("t") if "t" in capture else False
        tail = ("松开后停止成立（后续 toggle 段内 0 个 h）" if stopped
                else "松开后是否停止：后续仍出现 h 或没有 toggle 段，需人工确认")
        lines.append(f"hold：{h} 次 → 按住期间循环播放成立；{tail}")
    if t == 0:
        lines.append("toggle：0 次 → 未捕获到 t。若确实点过前进键，说明 fn=5 物理上不触发宏；"
                     "也可能是本轮根本没按该键，需结合人工确认。")
    elif t == 1:
        lines.append("toggle：1 次 → 只播放一次，未实现开关式循环。")
    else:
        lines.append(f"toggle：{t} 次 → 点一次后自行循环播放成立。")
        if idle_s is not None and idle_s >= 3.0:
            lines.append(f"toggle 停止：收尾前 {idle_s:.1f}s 无新字符 → 第二次点击已停下。")
        else:
            lines.append("toggle 停止：本轮由脚本强制还原（bind off）掐断，"
                         "未据此声称第二次点击已停；如需验证，收尾前留 ≥3s 空档。")
    return "\n".join(lines)


def selftest() -> int:
    """用本轮真实采集串离线验证判读逻辑，不碰设备。"""
    cap = "chhhhhh" + "t" * 36
    ok = 0
    v = verdict(cap, 0.4)
    for needle in ("对照组 c>=1", "hold：6 次 → 按住期间循环播放成立",
                   "松开后停止成立", "toggle：36 次 → 点一次后自行循环播放成立",
                   "由脚本强制还原"):
        assert needle in v, f"missing: {needle}"
        ok += 1
    v2 = verdict(cap, 5.0)
    assert "第二次点击已停下" in v2
    ok += 1
    for bad, needle in (("", "不可判读"), ("c", "hold：0 次"),
                        ("ch" + "t" * 3, "hold：1 次"), ("c" + "h" * 3, "toggle：0 次")):
        assert needle in verdict(bad, 0), f"missing: {needle}"
        ok += 1
    print(f"holdtest verdict selftest: {ok}/{ok} checks passed")
    return 0


def watch() -> None:
    """零依赖观察窗：本机 Python 不带 tkinter，直接用 user32 建控件。

    一个多行 Edit 接收宏打出的字符，WM_TIMER 每 200ms 读回内容并计数；
    按钮 / 自动判定 / 超时三条路径都会走到 finish()，保证一定会还原设备。
    """
    import ctypes as C

    u32, k32, g32 = C.windll.user32, C.windll.kernel32, C.windll.gdi32
    HWND, LRESULT = C.c_void_p, C.c_ssize_t
    WNDPROC = C.WINFUNCTYPE(LRESULT, HWND, C.c_uint, C.c_size_t, C.c_ssize_t)

    class WNDCLASSEXW(C.Structure):
        _fields_ = [
            ("cbSize", C.c_uint), ("style", C.c_uint), ("lpfnWndProc", WNDPROC),
            ("cbClsExtra", C.c_int), ("cbWndExtra", C.c_int), ("hInstance", HWND),
            ("hIcon", HWND), ("hCursor", HWND), ("hbrBackground", HWND),
            ("lpszMenuName", C.c_wchar_p), ("lpszClassName", C.c_wchar_p),
            ("hIconSm", HWND),
        ]

    class MSG(C.Structure):
        _fields_ = [
            ("hwnd", HWND), ("message", C.c_uint), ("wParam", C.c_size_t),
            ("lParam", C.c_ssize_t), ("time", C.c_uint), ("pt", C.c_long * 2),
        ]

    # 64 位下必须显式声明签名，否则句柄会被截成 32 位
    u32.DefWindowProcW.argtypes = [HWND, C.c_uint, C.c_size_t, C.c_ssize_t]
    u32.DefWindowProcW.restype = LRESULT
    u32.RegisterClassExW.argtypes = [C.POINTER(WNDCLASSEXW)]
    u32.RegisterClassExW.restype = C.c_ushort
    u32.CreateWindowExW.argtypes = [C.c_uint, C.c_wchar_p, C.c_wchar_p, C.c_uint,
                                    C.c_int, C.c_int, C.c_int, C.c_int,
                                    HWND, HWND, HWND, C.c_void_p]
    u32.CreateWindowExW.restype = HWND
    u32.SendMessageW.argtypes = [HWND, C.c_uint, C.c_size_t, C.c_void_p]
    u32.SendMessageW.restype = LRESULT
    u32.SetWindowTextW.argtypes = [HWND, C.c_wchar_p]
    u32.SetWindowTextW.restype = C.c_int
    u32.GetMessageW.argtypes = [C.POINTER(MSG), HWND, C.c_uint, C.c_uint]
    u32.GetMessageW.restype = C.c_int
    for name, args in [
        ("SetFocus", [HWND]), ("SetForegroundWindow", [HWND]),
        ("BringWindowToTop", [HWND]), ("UpdateWindow", [HWND]),
        ("ShowWindow", [HWND, C.c_int]), ("DestroyWindow", [HWND]),
        ("TranslateMessage", [C.POINTER(MSG)]), ("DispatchMessageW", [C.POINTER(MSG)]),
        ("PostQuitMessage", [C.c_int]), ("KillTimer", [HWND, C.c_size_t]),
        ("SetWindowPos", [HWND, HWND, C.c_int, C.c_int, C.c_int, C.c_int, C.c_uint]),
        ("SetTimer", [HWND, C.c_size_t, C.c_uint, C.c_void_p]),
    ]:
        getattr(u32, name).argtypes = args
    k32.GetModuleHandleW.argtypes = [C.c_wchar_p]
    k32.GetModuleHandleW.restype = HWND
    g32.GetStockObject.argtypes = [C.c_int]
    g32.GetStockObject.restype = C.c_void_p

    WS_OVERLAPPEDWINDOW, WS_CHILD, WS_VISIBLE = 0x00CF0000, 0x40000000, 0x10000000
    WS_BORDER, WS_VSCROLL, WS_TABSTOP = 0x00800000, 0x00200000, 0x00010000
    ES_MULTILINE, ES_AUTOVSCROLL, ES_WANTRETURN = 0x0004, 0x0080, 0x1000
    WM_GETTEXT, WM_SETFONT = 0x000D, 0x0030
    WM_COMMAND, WM_TIMER, WM_DESTROY = 0x0111, 0x0113, 0x0002
    CID_EDIT, CID_BTN = 1001, 1003

    state = {"capture": "", "finished": False, "start": time.monotonic(),
             "last_change": time.monotonic(), "last_len": 0, "last_log": 0.0,
             "result": ""}

    def read_edit() -> str:
        buf = C.create_unicode_buffer(8192)
        u32.SendMessageW(state["hEdit"], WM_GETTEXT, 8192, C.cast(buf, C.c_void_p))
        return buf.value

    def finish() -> None:
        if state["finished"]:
            return
        state["finished"] = True
        u32.KillTimer(state["hwnd"], 1)
        u32.SetWindowTextW(state["hHint"], "正在停止宏并还原设备，请稍候…")
        u32.UpdateWindow(state["hwnd"])
        now = time.monotonic()
        status = restore(snap)
        idle = now - state["last_change"]
        state["result"] = ("\n=== 验收结果 ===\n" + verdict(state["capture"], idle)
                           + f"\n还原状态：{status}\n")
        u32.DestroyWindow(state["hwnd"])

    def tick() -> None:
        if state["finished"]:
            return
        cap = read_edit()
        state["capture"] = cap
        label = f"已捕获   c={cap.count('c')}   h={cap.count('h')}   t={cap.count('t')}"
        u32.SetWindowTextW(state["hCounter"], label)
        u32.UpdateWindow(state["hCounter"])
        now = time.monotonic()
        if len(cap) != state["last_len"]:
            state["last_len"] = len(cap)
            state["last_change"] = now
        elapsed, idle = now - state["start"], now - state["last_change"]
        # 每 2 秒同步一次进展，外部可以实时读到按键结果，不必等收尾
        if now - state["last_log"] > 2:
            state["last_log"] = now
            print(f"[watch] c={cap.count('c')} h={cap.count('h')} t={cap.count('t')} "
                  f"elapsed={elapsed:.0f}s", flush=True)
        auto = all(ch in cap for ch in ("c", "h", "t")) and idle > IDLE_FINISH_S
        if auto or elapsed > TIMEOUT_S:
            print(f"[watch] 自动收尾（elapsed={elapsed:.0f}s idle={idle:.0f}s）", flush=True)
            finish()

    def wndproc(hwnd, msg, wparam, lparam):
        if msg == WM_COMMAND and (wparam & 0xFFFF) == CID_BTN:
            finish()
        elif msg == WM_TIMER and wparam == 1:
            tick()
        elif msg == WM_DESTROY:
            u32.PostQuitMessage(0)
        return u32.DefWindowProcW(hwnd, msg, wparam, lparam)

    snap = baseline()
    prepare()

    hinst = k32.GetModuleHandleW(None)
    wc = WNDCLASSEXW()
    wc.cbSize = C.sizeof(WNDCLASSEXW)
    wc.lpfnWndProc = WNDPROC(wndproc)
    wc.hInstance = hinst
    wc.hbrBackground = HWND(6)  # COLOR_WINDOW + 1
    wc.lpszClassName = "RazerMateHoldTest"
    if not u32.RegisterClassExW(C.byref(wc)):
        print("[watch] RegisterClassExW 失败", flush=True)
        restore(snap)
        return
    state["wndproc"] = wc.lpfnWndProc  # 防 GC

    hwnd = u32.CreateWindowExW(
        0, "RazerMateHoldTest", "RazerMate · hold / toggle 物理按键验收",
        WS_OVERLAPPEDWINDOW, 140, 100, 850, 580, None, None, hinst, None)
    if not hwnd:
        print("[watch] CreateWindowExW 失败", flush=True)
        restore(snap)
        return
    state["hwnd"] = hwnd

    def child(cls: str, text: str, style: int, x: int, y: int, w: int, h: int, cid: int):
        return u32.CreateWindowExW(0, cls, text, WS_CHILD | WS_VISIBLE | style,
                                   x, y, w, h, hwnd, HWND(cid), hinst, None)

    g32.CreateFontW.argtypes = [C.c_int] * 5 + [C.c_uint32] * 8 + [C.c_wchar_p]
    g32.CreateFontW.restype = C.c_void_p
    font = g32.CreateFontW(-15, 0, 0, 0, 400, 0, 0, 0, 134, 0, 0, 5, 0,
                           "Microsoft YaHei") or g32.GetStockObject(17)

    handles = []
    for i, line in enumerate(INSTRUCTION_LINES):
        h = child("Static", line, 0, 14, 10 + i * 24, 810, 22, 1100 + i)
        handles.append(h)
        u32.SendMessageW(h, WM_SETFONT, font, C.c_void_p(1))
    state["hHint"] = handles[0]

    state["hCounter"] = child("Static", "已捕获   c=0   h=0   t=0", 0,
                              14, 254, 810, 26, 1002)
    state["hEdit"] = child("Edit", "", WS_BORDER | WS_VSCROLL | WS_TABSTOP |
                           ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
                           14, 286, 810, 160, CID_EDIT)
    state["hBtn"] = child("Button", "结束并还原（测完点这里）", 0,
                          14, 456, 810, 46, CID_BTN)

    for h in (state["hCounter"], state["hEdit"], state["hBtn"]):
        u32.SendMessageW(h, WM_SETFONT, font, C.c_void_p(1))

    u32.ShowWindow(hwnd, 5)
    u32.SetWindowPos(hwnd, HWND(-1), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0040)  # TOPMOST
    u32.BringWindowToTop(hwnd)
    u32.SetForegroundWindow(hwnd)
    u32.SetFocus(state["hEdit"])
    u32.SetTimer(hwnd, 1, 200, None)
    print("[watch] 观察窗已就绪，等待按键…", flush=True)

    msg = MSG()
    while True:
        r = u32.GetMessageW(C.byref(msg), None, 0, 0)
        if r <= 0:
            break
        u32.TranslateMessage(C.byref(msg))
        u32.DispatchMessageW(C.byref(msg))
    print(state["result"], flush=True)


def main() -> int:
    arg = sys.argv[1] if len(sys.argv) > 1 else "--watch"
    if arg != "--selftest" and not CTL.exists():
        print(f"找不到 {CTL}，请先跑 versions/v1-gui/scripts/package.ps1 生成 release 交付目录")
        return 2
    if arg == "--prepare":
        baseline()
        prepare()
    elif arg == "--restore":
        restore()
    elif arg == "--selftest":
        return selftest()
    elif arg == "--watch":
        watch()
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
