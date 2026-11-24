"""把 RazerMate 窗口内容滚到底并截图。

灯光页 / 设置页的卡片比 680px 视口高，验证下方控件时得先滚。
单独一个脚本，保证「提窗 → 滚轮 → 截图」在同一个进程里连着做完。
"""

import ctypes
import ctypes.wintypes as wt
import sys
import time

from PIL import ImageGrab

try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass

user32 = ctypes.windll.user32
MOUSEEVENTF_WHEEL = 0x0800


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


def main() -> int:
    part, out = sys.argv[1], sys.argv[2]
    steps = int(sys.argv[3]) if len(sys.argv) > 3 else 6
    hwnd, title = find_window(part)
    if not hwnd:
        print(f"未找到窗口 {part!r}")
        return 1

    SWP_NOMOVE, SWP_NOSIZE = 0x0002, 0x0001
    user32.ShowWindow(hwnd, 9)
    user32.SetWindowPos(hwnd, -1, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)
    user32.SetWindowPos(hwnd, -2, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.5)

    pt = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    user32.SetCursorPos(pt.x + 600, pt.y + 430)
    time.sleep(0.2)

    user32.mouse_event.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD, wt.DWORD, ctypes.c_void_p]
    for _ in range(steps):
        user32.mouse_event(MOUSEEVENTF_WHEEL, 0, 0, ctypes.c_uint(-120).value, 0)
        time.sleep(0.12)
    time.sleep(0.5)

    im = ImageGrab.grab(bbox=(pt.x, pt.y, pt.x + 980, pt.y + 680), all_screens=True)
    im.save(out)
    print(f"{title!r} 客户区原点 ({pt.x},{pt.y}) → {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
