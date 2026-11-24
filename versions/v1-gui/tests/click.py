"""开发期辅助脚本：向窗口发一次真实鼠标点击，用来验证 Slint 的命中测试。

坐标以「窗口左上角 + 截图内像素」给出，脚本自己加上窗口原点。
"""

import ctypes
import ctypes.wintypes as wt
import sys
import time

def _enable_dpi_awareness():
    """让进程按物理像素工作，否则 GetWindowRect 拿到的是被缩放的逻辑坐标，
    和 ImageGrab 抓出来的物理像素对不上（150% 缩放时整个截图会偏掉）。"""
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)  # PER_MONITOR_AWARE
    except Exception:
        try:
            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass


_enable_dpi_awareness()


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
    time.sleep(0.15)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.08)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)


def main() -> int:
    part = sys.argv[1]
    ix, iy = int(sys.argv[2]), int(sys.argv[3])

    hwnd, title = find_window(part)
    if not hwnd:
        print(f"未找到窗口 {part!r}")
        return 1

    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    sx, sy = rect.left + ix, rect.top + iy
    print(f"窗口 {title!r} 原点 ({rect.left},{rect.top})，点击窗口内 ({ix},{iy}) → 屏幕 ({sx},{sy})")

    user32.SetForegroundWindow(hwnd)
    time.sleep(0.4)
    click(sx, sy)
    time.sleep(0.8)
    print("已点击")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
