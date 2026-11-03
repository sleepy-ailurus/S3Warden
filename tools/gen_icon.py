#!/usr/bin/env python3
"""生成 S3Warden 应用图标：渐变底 + 白色存储桶（bucket）剪影。

仅使用标准库（struct / zlib / math），不依赖 Pillow。
输出 src-tauri/icons/ 下的 icon.ico 与若干 PNG，供 tauri-build 的 Windows 资源编译使用。

用法：
    python tools/gen_icon.py
"""

from __future__ import annotations

import math
import os
import struct
import zlib

# 渐变背景（深蓝 → 亮蓝），与前端主色调一致
BG_TOP = (30, 58, 138)
BG_BOTTOM = (37, 99, 235)
FG = (255, 255, 255)

# 圆角矩形参数（归一化坐标，范围 [-1, 1]）
RECT_HALF = 0.94
CORNER_R = 0.24

# 桶身几何
LID_TOP, LID_BOTTOM = -0.52, -0.38  # 桶盖（桶沿）
LID_HALF = 0.62
BODY_TOP, BODY_BOTTOM = -0.38, 0.56  # 桶身（上宽下窄）
BODY_HALF_TOP, BODY_HALF_BOTTOM = 0.54, 0.40
STRIPE_HALF = 0.045  # 桶身镂空横条纹半高
STRIPE_YS = (-0.09, 0.22)
STRIPE_INSET = 0.13


def in_rounded_rect(x: float, y: float) -> bool:
    ax, ay = abs(x), abs(y)
    if ax > RECT_HALF or ay > RECT_HALF:
        return False
    cx = max(ax - (RECT_HALF - CORNER_R), 0.0)
    cy = max(ay - (RECT_HALF - CORNER_R), 0.0)
    return cx * cx + cy * cy <= CORNER_R * CORNER_R


def body_half_width(y: float) -> float | None:
    """桶身在高度 y 处的半宽；不在桶身范围返回 None。"""
    if y < BODY_TOP or y > BODY_BOTTOM:
        return None
    t = (y - BODY_TOP) / (BODY_BOTTOM - BODY_TOP)
    return BODY_HALF_TOP + (BODY_HALF_BOTTOM - BODY_HALF_TOP) * t


def in_bucket(x: float, y: float) -> bool:
    """桶 = 桶盖（稍宽的沿）+ 上宽下窄的桶身。"""
    if LID_TOP <= y <= LID_BOTTOM and abs(x) <= LID_HALF:
        return True
    w = body_half_width(y)
    return w is not None and abs(x) <= w


def in_stripe(x: float, y: float) -> bool:
    """桶身上的镂空横条纹（用背景色画出，代表分层存放的对象）。"""
    for sy in STRIPE_YS:
        if abs(y - sy) > STRIPE_HALF:
            continue
        w = body_half_width(y)
        if w is not None and abs(x) <= w - STRIPE_INSET:
            return True
    return False


def lerp(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


def render(size: int, ss: int = 3) -> bytearray:
    """渲染 size×size 的 RGBA 像素数据（ss×ss 超采样抗锯齿）。"""
    out = bytearray(size * size * 4)
    for py in range(size):
        for px in range(size):
            ar = ag = ab = 0
            aa = 0
            for sy in range(ss):
                for sx in range(ss):
                    # 像素中心 + 子采样偏移，映射到 [-1, 1]
                    nx = ((px + (sx + 0.5) / ss) / size) * 2 - 1
                    ny = ((py + (sy + 0.5) / ss) / size) * 2 - 1
                    if not in_rounded_rect(nx, ny):
                        continue
                    t = (ny + 1) / 2
                    r, g, b = lerp(BG_TOP, BG_BOTTOM, t)
                    if in_bucket(nx, ny) and not in_stripe(nx, ny):
                        r, g, b = FG
                    ar += r
                    ag += g
                    ab += b
                    aa += 255
            n = ss * ss
            i = (py * size + px) * 4
            if aa == 0:
                out[i : i + 4] = bytes(4)
                continue
            alpha = aa / n
            # 颜色按实际覆盖比例还原（预乘 → 直乘）
            out[i] = min(255, int(round(ar / aa * 255)))
            out[i + 1] = min(255, int(round(ag / aa * 255)))
            out[i + 2] = min(255, int(round(ab / aa * 255)))
            out[i + 3] = int(round(alpha))
    return out


def png_bytes(size: int) -> bytes:
    data = render(size)
    raw = bytearray()
    for y in range(size):
        raw.append(0)  # filter type: None
        raw += data[y * size * 4 : (y + 1) * size * 4]

    def chunk(tag: bytes, body: bytes) -> bytes:
        return (
            struct.pack(">I", len(body))
            + tag
            + body
            + struct.pack(">I", zlib.crc32(tag + body) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def ico_bytes(sizes: list[int]) -> bytes:
    images = [(s, png_bytes(s)) for s in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for size, blob in images:
        entries += struct.pack(
            "<BBBBHHII",
            size % 256,  # width（256 记为 0）
            size % 256,  # height
            0,  # color count
            0,  # reserved
            1,  # planes
            32,  # bit count
            len(blob),
            offset,
        )
        offset += len(blob)
    return header + entries + b"".join(blob for _, blob in images)


def main() -> None:
    here = os.path.dirname(os.path.abspath(__file__))
    icons = os.path.normpath(os.path.join(here, "..", "src-tauri", "icons"))
    os.makedirs(icons, exist_ok=True)

    sizes = [16, 32, 48, 64, 128, 256]
    with open(os.path.join(icons, "icon.ico"), "wb") as f:
        f.write(ico_bytes(sizes))
    for size in (256, 512):
        with open(os.path.join(icons, f"{size}x{size}.png"), "wb") as f:
            f.write(png_bytes(size))
    with open(os.path.join(icons, "icon.png"), "wb") as f:
        f.write(png_bytes(512))
    # Tauri 惯例文件名
    for name, size in (("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)):
        with open(os.path.join(icons, name), "wb") as f:
            f.write(png_bytes(size))
    print(f"icons written to {icons}")


if __name__ == "__main__":
    main()
