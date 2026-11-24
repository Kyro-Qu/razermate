"""把 RazerMate 窗口提到最前然后截它的客户区。

单独一个脚本是必要的：提窗和截图必须在同一个进程里连着做完，
否则中间别的窗口可能又盖上来，截到的是别人。
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
    part = sys.argv[1]
    out = sys.argv[2]
    w = int(sys.argv[3]) if len(sys.argv) > 3 else 980
    h = int(sys.argv[4]) if len(sys.argv) > 4 else 680

    hwnd, title = find_window(part)
    if not hwnd:
        print(f"未找到窗口 {part!r}")
        return 1

    SWP_NOMOVE, SWP_NOSIZE = 0x0002, 0x0001
    user32.ShowWindow(hwnd, 9)
    user32.SetWindowPos(hwnd, -1, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)
    user32.SetWindowPos(hwnd, -2, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.6)

    pt = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))

    from PIL import ImageGrab

    im = ImageGrab.grab(bbox=(pt.x, pt.y, pt.x + w, pt.y + h), all_screens=True)
    im.save(out)
    print(f"{title!r} 客户区原点 ({pt.x},{pt.y}) → {out} {im.size}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
