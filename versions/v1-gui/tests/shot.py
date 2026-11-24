"""开发期辅助脚本：截取 razermate 窗口，用来核对界面渲染。

只是工具，不参与打包，也不被 Rust 侧引用。

坐标说明：本机是多显示器 + 混合 DPI，GetWindowRect / DWM 边框 / ImageGrab 三者
的坐标空间对不上（相差 1.5×），按 rect 裁剪会连桌面一起抓进来。所以这里改成
「整屏抓 → 按窗口背景色定位边界 → 裁掉多余部分」，不依赖任何窗口几何 API。
"""

import ctypes
import ctypes.wintypes as wt
import sys
import time

from PIL import ImageGrab


def _enable_dpi_awareness():
    """让进程按物理像素工作，尽量让 ImageGrab 与屏幕一致。"""
    try:
        ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
        return
    except Exception:
        pass
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except Exception:
        try:
            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass


_enable_dpi_awareness()

user32 = ctypes.windll.user32

# 窗口底色 Theme.bg = #15161a。只认这个颜色，别的深色窗口不会误判。
BG = (0x15, 0x16, 0x1A)
TOL = 6


def find_window(title_part: str):
    found = []

    @ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
    def cb(hwnd, _):
        if not user32.IsWindowVisible(hwnd):
            return True
        n = user32.GetWindowTextLengthW(hwnd)
        if n == 0:
            return True
        buf = ctypes.create_unicode_buffer(n + 1)
        user32.GetWindowTextW(hwnd, buf, n + 1)
        if title_part in buf.value:
            found.append((hwnd, buf.value))
        return True

    user32.EnumWindows(cb, 0)
    return found[0] if found else (None, None)


def app_bbox(img):
    """在整屏截图里找窗口背景色的外接矩形。"""
    px = img.load()
    w, h = img.size
    minx, miny, maxx, maxy = w, h, -1, -1
    # 隔行采样，几百万像素逐点扫太慢。
    for y in range(0, h, 2):
        for x in range(0, w, 2):
            r, g, b = px[x, y][:3]
            if abs(r - BG[0]) <= TOL and abs(g - BG[1]) <= TOL and abs(b - BG[2]) <= TOL:
                if x < minx:
                    minx = x
                if y < miny:
                    miny = y
                if x > maxx:
                    maxx = x
                if y > maxy:
                    maxy = y
    if maxx < 0:
        return None
    return (minx, miny, maxx + 2, maxy + 2)


def main() -> int:
    part = sys.argv[1] if len(sys.argv) > 1 else "RazerMate"
    out = sys.argv[2] if len(sys.argv) > 2 else "shot.png"

    hwnd, title = find_window(part)
    if not hwnd:
        print(f"未找到标题含 {part!r} 的窗口")
        return 1

    user32.SetForegroundWindow(hwnd)
    time.sleep(0.8)

    full = ImageGrab.grab(all_screens=True)
    box = app_bbox(full)
    if not box:
        print("没在屏幕里找到窗口背景色，直接存整屏")
        full.save(out)
        print(f"已保存 {out}  {full.size[0]}x{full.size[1]}")
        return 0

    img = full.crop(box)
    img.save(out)
    print(f"窗口: {title!r}  裁剪 {box}  已保存 {out}  {img.size[0]}x{img.size[1]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
