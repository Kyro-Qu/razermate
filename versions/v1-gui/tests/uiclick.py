"""向 RazerMate 窗口发真实鼠标点击，坐标以「客户区像素」给出。

为什么不用 GetWindowRect：这台机器 150% 缩放 + 多显示器，GetWindowRect 给出的
坐标和实际像素对不上。ClientToScreen 直接问系统「客户区原点在哪」，不受缩放影响。

用法：
    python versions/v1-gui/tests/uiclick.py "RazerMate" 789,240 310,284 900,240

每个参数是 `x,y`，按顺序逐个点击，点之间留出界面响应时间。
"""

import ctypes
import ctypes.wintypes as wt
import sys
import time


def _dpi_aware():
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except Exception:
        try:
            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass


_dpi_aware()
user32 = ctypes.windll.user32
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004


def find_window(part: str):
    found = []

    @ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
    def cb(hwnd, _):
        if user32.IsWindowVisible(hwnd):
            n = user32.GetWindowTextLengthW(hwnd)
            if n:
                buf = ctypes.create_unicode_buffer(n + 1)
                user32.GetWindowTextW(hwnd, buf, n + 1)
                if part in buf.value:
                    found.append((hwnd, buf.value))
        return True

    user32.EnumWindows(cb, 0)
    return found[0] if found else (None, None)


def click(x: int, y: int):
    user32.SetCursorPos(x, y)
    time.sleep(0.12)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.06)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)


WM_CHAR = 0x0102
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004


class _KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wt.WORD),
        ("wScan", wt.WORD),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    ]


class _INPUTUNION(ctypes.Union):
    # 必须按最大的成员（MOUSEINPUT，32 字节）补齐大小：
    # 给少了 SendInput 会以 ERROR_INVALID_PARAMETER 静默失败。
    _fields_ = [("ki", _KEYBDINPUT), ("pad", ctypes.c_byte * 32)]


class _INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", _INPUTUNION)]


def type_text(hwnd, text: str):
    """往里敲字。

    试过直接 PostMessage(WM_CHAR)，winit 不认——它的文本输入走的是真实
    键盘路径。所以这里用 SendInput + KEYEVENTF_UNICODE 注入字符，
    系统会照常生成 WM_KEYDOWN/WM_CHAR，与真按键盘等价，且不受输入法影响。
    """
    for ch in text:
        for flags in (KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP):
            inp = _INPUT(
                type=1,  # INPUT_KEYBOARD
                u=_INPUTUNION(
                    ki=_KEYBDINPUT(
                        wVk=0,
                        wScan=ord(ch),
                        dwFlags=flags,
                        time=0,
                        dwExtraInfo=0,
                    )
                ),
            )
            n = user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(_INPUT))
            if n != 1:
                raise OSError(
                    f"SendInput 失败，GetLastError={ctypes.get_last_error()}，"
                    f"结构大小={ctypes.sizeof(_INPUT)}"
                )
            time.sleep(0.015)
        time.sleep(0.02)
    time.sleep(0.3)


def send_vk(vk: int, up: bool = False):
    """发一个虚拟键。修饰键靠调用方自己按下/抬起。"""
    flags = KEYEVENTF_KEYUP if up else 0
    inp = _INPUT(
        type=1,
        u=_INPUTUNION(
            ki=_KEYBDINPUT(wVk=vk, wScan=0, dwFlags=flags, time=0, dwExtraInfo=0)
        ),
    )
    user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(_INPUT))
    time.sleep(0.02)


VK_CONTROL = 0x11
VK_A = 0x41
VK_SHIFT = 0x10
VK_MENU = 0x12  # Alt
VK_LWIN = 0x5B

_MOD_VK = {"ctrl": VK_CONTROL, "shift": VK_SHIFT, "alt": VK_MENU, "win": VK_LWIN}
_VK_NAMED = {
    "tab": 0x09, "enter": 0x0D, "space": 0x20, "esc": 0x1B,
    "backspace": 0x08, "delete": 0x2E, "home": 0x24, "end": 0x23,
    "pageup": 0x21, "pagedown": 0x22,
    "left": 0x25, "up": 0x26, "right": 0x27, "down": 0x28,
}
for _i in range(1, 25):
    _VK_NAMED[f"f{_i}"] = 0x6F + _i  # F1=0x70


def send_chord(spec: str):
    """按下一个组合键，例如 ctrl+c / ctrl+shift+z / alt+tab。

    专供 GUI「录入组合键」验收：修饰键先按下，主键按下+松开，修饰键再松开，
    这样 Slint 的 KeyEvent.modifiers 里能看到修饰键状态。
    """
    parts = [p.strip().lower() for p in spec.split("+") if p.strip()]
    mods, mains = [], []
    for p in parts:
        if p in _MOD_VK:
            mods.append(_MOD_VK[p])
        else:
            mains.append(_VK_NAMED.get(p) or (ord(p.upper()) if len(p) == 1 else None))
    if not mains or any(v is None for v in mains):
        raise ValueError(f"无法解析组合键 {spec!r}")
    for vk in mods:
        send_vk(vk)
    for vk in mains:
        send_vk(vk)
    for vk in reversed(mains):
        send_vk(vk, up=True)
    for vk in reversed(mods):
        send_vk(vk, up=True)
    time.sleep(0.15)


def select_all():
    """Ctrl+A。用来先清空再输入，免得新内容接在旧内容后面。"""
    send_vk(VK_CONTROL)
    send_vk(VK_A)
    send_vk(VK_A, up=True)
    send_vk(VK_CONTROL, up=True)
    time.sleep(0.1)


def raise_window(hwnd):
    """把窗口真正提到最前。

    只调 SetForegroundWindow 是不够的：Windows 有前台锁，别的进程调用经常被静默
    拒绝，于是点击会落到压在它上面的窗口上。TOPMOST 一上一下这一招不受前台锁影响。
    """
    SWP_NOMOVE, SWP_NOSIZE = 0x0002, 0x0001
    HWND_TOPMOST, HWND_NOTOPMOST = -1, -2
    user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    user32.SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)
    user32.SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.5)


def main() -> int:
    part = sys.argv[1]
    hwnd, title = find_window(part)
    if not hwnd:
        print(f"未找到窗口 {part!r}")
        return 1

    raise_window(hwnd)

    pt = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    print(f"窗口 {title!r}，客户区原点 ({pt.x},{pt.y})")

    for spec in sys.argv[2:]:
        if spec.startswith("t:"):
            payload = spec[2:]
            print(f"输入 {payload!r}")
            type_text(hwnd, payload)
            continue
        if spec.startswith("k:"):
            chord = spec[2:]
            print(f"按组合键 {chord}")
            send_chord(chord)
            continue
        if spec == "a":
            print("全选（Ctrl+A）")
            select_all()
            continue
        ix, iy = (int(v) for v in spec.split(","))
        sx, sy = pt.x + ix, pt.y + iy
        print(f"点击 ({ix},{iy}) → 屏幕 ({sx},{sy})")
        click(sx, sy)
        time.sleep(0.9)

    print("完成")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
