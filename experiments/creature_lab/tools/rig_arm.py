#!/usr/bin/env python3
"""rig_arm.py — fully code-driven 2-bone arm rig for a static creature image. No diffusion, no
tracking: we know exactly where the arm is every frame because we're drawing it ourselves. Draws a
tapered shoulder->elbow->wrist arm (matching mkslash3's proportions/timing) directly onto the body
image (after erasing the original resting paw), and emits an anchors.txt (attach.rs format) so the
existing `attach` binary can composite a crisp weapon sprite onto the wrist with the correct angle —
zero melting risk, zero floating-prop risk, since geometry is exact every frame.

Usage: rig_arm.py <body_image> <out_frames_dir> <out_anchors.txt> [frames] [--erase x0,y0,x1,y1]
"""
import argparse
import math
import os
from PIL import Image, ImageDraw, ImageFilter


def lerp(a, b, t):
    return a + (b - a) * t


def ease_in(u):
    return u * u


def ease_out_cubic(u):
    return 1.0 - (1.0 - u) ** 3


def capsule(draw, p0, p1, th0, th1, fill, outline, outline_w):
    """Tapered capsule from p0 (thickness th0) to p1 (thickness th1), with an outline stroke."""
    n = 12
    outline_poly_top = []
    outline_poly_bot = []
    dx, dy = p1[0] - p0[0], p1[1] - p0[1]
    length = math.hypot(dx, dy) or 1.0
    nx, ny = -dy / length, dx / length  # perpendicular
    for i in range(n + 1):
        t = i / n
        cx, cy = lerp(p0[0], p1[0], t), lerp(p0[1], p1[1], t)
        th = lerp(th0, th1, t) / 2.0
        outline_poly_top.append((cx + nx * th, cy + ny * th))
        outline_poly_bot.append((cx - nx * th, cy - ny * th))
    poly = outline_poly_top + outline_poly_bot[::-1]
    # outline pass (slightly larger), then fill pass on top
    big = []
    pad = outline_w
    for i in range(n + 1):
        t = i / n
        cx, cy = lerp(p0[0], p1[0], t), lerp(p0[1], p1[1], t)
        th = lerp(th0, th1, t) / 2.0 + pad
        big.append((cx + nx * th, cy + ny * th))
    big_bot = []
    for i in range(n + 1):
        t = i / n
        cx, cy = lerp(p0[0], p1[0], t), lerp(p0[1], p1[1], t)
        th = lerp(th0, th1, t) / 2.0 + pad
        big_bot.append((cx - nx * th, cy - ny * th))
    draw.polygon(big + big_bot[::-1], fill=outline)
    draw.ellipse([p0[0] - th0 / 2 - pad, p0[1] - th0 / 2 - pad, p0[0] + th0 / 2 + pad, p0[1] + th0 / 2 + pad], fill=outline)
    draw.ellipse([p1[0] - th1 / 2 - pad, p1[1] - th1 / 2 - pad, p1[0] + th1 / 2 + pad, p1[1] + th1 / 2 + pad], fill=outline)
    draw.polygon(poly, fill=fill)
    draw.ellipse([p0[0] - th0 / 2, p0[1] - th0 / 2, p0[0] + th0 / 2, p0[1] + th0 / 2], fill=fill)
    draw.ellipse([p1[0] - th1 / 2, p1[1] - th1 / 2, p1[0] + th1 / 2, p1[1] + th1 / 2], fill=fill)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("body_image")
    ap.add_argument("out_frames_dir")
    ap.add_argument("out_anchors")
    ap.add_argument("frames", nargs="?", type=int, default=25)
    ap.add_argument("--erase", default=None, help="x0,y0,x1,y1 region to erase (the original resting paw)")
    ap.add_argument("--fur", default=None, help="R,G,B fur color override; sampled from image if omitted")
    ap.add_argument("--outline", default="35,18,10", help="R,G,B outline color")
    args = ap.parse_args()

    body = Image.open(args.body_image).convert("RGB")
    w, h = body.size

    if args.erase:
        x0, y0, x1, y1 = (int(v) for v in args.erase.split(","))
        # sample local background from a ring just outside the erase box (matches the green
        # screen's own vignette/gradient far better than one fixed corner color would), then
        # feather the erase mask so it blends instead of leaving a hard rectangle seam.
        pad = 6
        ring = [
            body.getpixel((x, max(0, y0 - pad))) for x in range(x0, x1, 4)
        ] + [
            body.getpixel((x, min(h - 1, y1 + pad))) for x in range(x0, x1, 4)
        ] + [
            body.getpixel((max(0, x0 - pad), y)) for y in range(y0, y1, 4)
        ]
        local_bg = tuple(sum(c) // len(ring) for c in zip(*ring))

        patch = Image.new("RGB", (x1 - x0, y1 - y0), local_bg)
        mask = Image.new("L", (x1 - x0, y1 - y0), 255)
        mask = mask.filter(ImageFilter.GaussianBlur(0))
        # feather via a smaller-than-box white ellipse blurred to soft edges
        mdraw = ImageDraw.Draw(mask)
        mdraw.rectangle([0, 0, x1 - x0, y1 - y0], fill=0)
        mdraw.ellipse([4, 4, (x1 - x0) - 4, (y1 - y0) - 4], fill=255)
        mask = mask.filter(ImageFilter.GaussianBlur(8))
        body.paste(patch, (x0, y0), mask)

    fur = tuple(int(v) for v in args.fur.split(",")) if args.fur else body.getpixel((int(0.5 * w), int(0.55 * h)))
    outline = tuple(int(v) for v in args.outline.split(","))

    os.makedirs(args.out_frames_dir, exist_ok=True)

    shx, shy = 0.40 * w, 0.62 * h
    l1, l2 = 0.11 * w, 0.13 * w
    t_sh, t_el, t_wr = 0.040 * w, 0.030 * w, 0.020 * w

    sh_rest, sh_wind, sh_strike = math.radians(100), math.radians(235), math.radians(55)
    el_rest, el_wind, el_strike = math.radians(-15), math.radians(-55), math.radians(-5)

    n = args.frames
    anchors = []
    for f in range(n):
        t = f / (n - 1) if n > 1 else 0.0
        if t < 0.4:
            u = ease_in(t / 0.4)
            sh_ang = lerp(sh_rest, sh_wind, u)
            el_ang = lerp(el_rest, el_wind, u)
        else:
            u = ease_out_cubic((t - 0.4) / 0.6)
            sh_ang = lerp(sh_wind, sh_strike, u)
            el_ang = lerp(el_wind, el_strike, u)

        ex, ey = shx + l1 * math.cos(sh_ang), shy + l1 * math.sin(sh_ang)
        wrist_ang = sh_ang + el_ang
        wx, wy = ex + l2 * math.cos(wrist_ang), ey + l2 * math.sin(wrist_ang)

        frame = body.copy()
        d = ImageDraw.Draw(frame)
        capsule(d, (shx, shy), (ex, ey), t_sh, t_el, fur, outline, 4)
        capsule(d, (ex, ey), (wx, wy), t_el, t_wr, fur, outline, 4)
        d.ellipse([wx - t_wr / 2, wy - t_wr / 2, wx + t_wr / 2, wy + t_wr / 2], fill=fur, outline=outline, width=3)
        frame.save(os.path.join(args.out_frames_dir, f"frame_{f:03d}.png"))

        dx, dy = wx - ex, wy - ey
        ang_deg = math.degrees(math.atan2(dx, -dy))
        anchors.append(f"{f} {wx:.1f} {wy:.1f} {ang_deg:.1f} {0.55}")

    with open(args.out_anchors, "w") as fh:
        fh.write("\n".join(anchors) + "\n")
    print(f"rig_arm: wrote {n} frames -> {args.out_frames_dir}, anchors -> {args.out_anchors}")


if __name__ == "__main__":
    main()
