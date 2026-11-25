"""Golden test: our bytes against the Python reference implementation.

Both sides build a 90-byte report for the same command; the bytes must match
exactly, CRC included. This catches layout drift without touching the mouse.

    python tests/golden_compare.py [path/to/razerctl.exe]
"""
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
DEFAULT_EXE = ROOT / "target" / "debug" / "razerctl.exe"
REF = Path(
    r"D:\WorkSpace\Project\DeviceManagement\ComputerManagement\razer\refs"
    r"\razerqdhid\public\py"
)

sys.path.insert(0, str(REF))
from qdrazer import protocol as pt  # noqa: E402

# (class, command, size, args) covering every command family we implement.
CASES = [
    ("firmware", 0x00, 0x81, 4, []),
    ("device mode read", 0x00, 0x84, 2, []),
    ("device mode write", 0x00, 0x04, 2, [0, 0]),
    ("binding read", 0x02, 0x8C, 10, [4, 5, 0]),
    ("binding write key", 0x02, 0x0C, 10, [4, 5, 0, 2, 2, 0x04, 0x1A, 0, 0, 0]),
    ("binding write macro", 0x02, 0x0C, 10, [4, 106, 0, 3, 3, 0x00, 0x01, 1, 0, 0]),
    ("profile count", 0x05, 0x8A, 1, []),
    ("profile list", 0x05, 0x81, 6, []),
    ("profile switch", 0x05, 0x04, 1, [3]),
    ("flash usage", 0x06, 0x8E, 14, []),
    ("macro count", 0x06, 0x80, 2, []),
    ("macro size", 0x06, 0x88, 6, [0x00, 0x01]),
    ("macro set size", 0x06, 0x08, 6, [0x00, 0x02, 0, 0, 0, 24]),
    ("macro read", 0x06, 0x89, 71, [0x00, 0x02, 0, 0, 0, 0, 64]),
    ("macro delete", 0x06, 0x03, 2, [0x00, 0x02]),
    ("led read", 0x0F, 0x82, 2, [4, 0x0A]),
    ("led static", 0x0F, 0x02, 9, [4, 0x0A, 1, 0, 0, 1, 0xFF, 0, 0]),
    ("led spectrum", 0x0F, 0x02, 6, [4, 0x0A, 3, 1, 0, 0]),
    ("led wave", 0x0F, 0x02, 6, [4, 0x0A, 4, 2, 0x60, 0]),
    ("brightness read", 0x0F, 0x84, 2, [4, 0x0A]),
    ("brightness write", 0x0F, 0x04, 3, [4, 0x0A, 0x99]),
]


def python_bytes(cls, cmd, size, args):
    r = pt.Report.new(cls, cmd, size)
    for i, v in enumerate(args):
        r.arguments[i] = v
    r.calculate_crc()
    return bytes(r)


def rust_bytes(exe, cls, cmd, size, args):
    cmdline = [
        str(exe),
        "hexdump",
        hex(cls),
        hex(cmd),
        str(size),
    ] + [hex(a) for a in args]
    out = subprocess.run(cmdline, capture_output=True, text=True)
    if out.returncode != 0:
        raise SystemExit(f"razerctl failed: {out.stderr.strip()}")
    return bytes.fromhex(out.stdout.strip())


def main():
    exe = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_EXE
    if not exe.exists():
        raise SystemExit(f"missing {exe}; run: cargo build")

    failures = 0
    for label, cls, cmd, size, args in CASES:
        py = python_bytes(cls, cmd, size, args)
        rs = rust_bytes(exe, cls, cmd, size, args)
        if py == rs:
            print(f"  ok    {label}")
        else:
            failures += 1
            print(f"  FAIL  {label}")
            if len(py) != len(rs):
                print(f"        length {len(py)} vs {len(rs)}")
            else:
                for i, (a, b) in enumerate(zip(py, rs)):
                    if a != b:
                        print(f"        byte[{i}] python={a:02x} rust={b:02x}")
                        break
                print(f"        py {py.hex()}")
                print(f"        rs {rs.hex()}")

    total = len(CASES)
    print(f"\n{total - failures}/{total} commands match byte for byte")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
