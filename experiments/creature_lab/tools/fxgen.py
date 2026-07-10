#!/usr/bin/env python3
"""fxgen.py — procedural (zero-diffusion-gamble) braille-pipeline attack effects.
Generalizable: not tied to any creature/weapon, just an anchor point + canvas size.
Renders to a green-screen-keyable RGB frame sequence (same convention as everything
else in this project — `playframes --chroma auto`).

Usage: fxgen.py <fire|crack|burst> <out_dir> [--frames 25] [--w 512] [--h 512]
                 [--anchor-x 0.5] [--anchor-y 0.62] [--scale 1.0] [--color R,G,B]
"""
import argparse
import math
import os
import numpy as np
from PIL import Image

BG = np.array([40, 220, 60], dtype=np.float32)  # green screen


def ease_out_cubic(t):
    t = np.clip(t, 0, 1)
    return 1 - (1 - t) ** 3


def ease_in_cubic(t):
    t = np.clip(t, 0, 1)
    return t ** 3


def fire_color_lut(intensity):
    """intensity in [0,1] -> RGB, black->darkred->orange->yellow->white."""
    stops = [
        (0.00, (10, 5, 5)),
        (0.25, (120, 20, 10)),
        (0.55, (235, 100, 20)),
        (0.80, (255, 200, 60)),
        (1.00, (255, 255, 230)),
    ]
    out = np.zeros(intensity.shape + (3,), dtype=np.float32)
    for i in range(len(stops) - 1):
        t0, c0 = stops[i]
        t1, c1 = stops[i + 1]
        mask = (intensity >= t0) & (intensity <= t1)
        span = max(t1 - t0, 1e-6)
        u = np.clip((intensity[mask] - t0) / span, 0, 1)
        for k in range(3):
            out[..., k][mask] = c0[k] + (c1[k] - c0[k]) * u
    return out


def gen_fire(out_dir, frames, w, h, ax, ay, scale):
    os.makedirs(out_dir, exist_ok=True)
    rng = np.random.default_rng(7)
    n_tongues = 6
    base_x = ax * w
    base_y = ay * h
    max_h = 0.55 * h * scale
    tongue_x = base_x + (rng.random(n_tongues) - 0.5) * 0.35 * w * scale
    tongue_w = (0.10 + rng.random(n_tongues) * 0.06) * w * scale
    tongue_phase = rng.random(n_tongues) * 10
    tongue_hscale = 0.65 + rng.random(n_tongues) * 0.5

    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)

    for i in range(frames):
        t = i / max(frames - 1, 1)
        # erupt (0-0.35), sustain/flicker (0.35-0.8), fade (0.8-1.0)
        if t < 0.35:
            growth = ease_out_cubic(t / 0.35)
        elif t < 0.8:
            growth = 1.0
        else:
            growth = 1.0 - ease_in_cubic((t - 0.8) / 0.2)

        intensity = np.zeros((h, w), dtype=np.float32)
        for k in range(n_tongues):
            flick = 0.85 + 0.15 * math.sin(t * 18 + tongue_phase[k])
            th = max_h * growth * tongue_hscale[k] * flick
            if th < 1:
                continue
            wobble = 0.18 * tongue_w[k] * np.sin(0.045 * (base_y - yy) + t * 14 + tongue_phase[k])
            cx = tongue_x[k] + wobble
            dy = np.clip((base_y - yy) / th, 0, None)
            taper = tongue_w[k] * (1.0 - 0.8 * np.clip(dy, 0, 1))
            dx = np.abs(xx - cx)
            active = (dy <= 1.0) & (dx < np.maximum(taper, 1e-3)) & (yy <= base_y + 4)
            local = np.zeros((h, w), dtype=np.float32)
            local[active] = (1.0 - dy[active]) * (1.0 - (dx[active] / np.maximum(taper[active], 1e-3)) ** 2)
            intensity = np.maximum(intensity, local)

        color = fire_color_lut(intensity)
        img = BG[None, None, :] * (1 - np.clip(intensity, 0, 1)[..., None]) + color * np.clip(intensity, 0, 1)[..., None]
        Image.fromarray(np.clip(img, 0, 255).astype(np.uint8)).save(os.path.join(out_dir, f"frame_{i:03d}.png"))
    print(f"fxgen fire: {frames} frames -> {out_dir}")


def fractal_branch(rng, x0, y0, angle, length, depth):
    """midpoint-displacement jagged line as a list of (x,y) points, deterministic given rng state."""
    pts = [(x0, y0)]
    x, y = x0, y0
    n_segs = 8
    for s in range(n_segs):
        angle += (rng.random() - 0.5) * 0.5
        step = length / n_segs
        x += step * math.cos(angle)
        y += step * math.sin(angle)
        pts.append((x, y))
    return pts


def gen_crack(out_dir, frames, w, h, ax, ay, scale):
    os.makedirs(out_dir, exist_ok=True)
    rng = np.random.default_rng(3)
    cx, cy = ax * w, ay * h
    n_branches = 7
    branches = []
    for b in range(n_branches):
        ang = (2 * math.pi / n_branches) * b + (rng.random() - 0.5) * 0.4
        length = (0.28 + rng.random() * 0.18) * w * scale
        branches.append(fractal_branch(rng, cx, cy, ang, length, 3))

    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)

    def dist_and_arclen_to_polyline(pts):
        """returns (distance-to-nearest-point, arc-length-of-that-nearest-point) per pixel."""
        d = None
        s = None
        acc = 0.0
        for i in range(len(pts) - 1):
            (x0, y0), (x1, y1) = pts[i], pts[i + 1]
            dx, dy = x1 - x0, y1 - y0
            seg_len = math.hypot(dx, dy)
            seg2 = max(dx * dx + dy * dy, 1e-6)
            proj = np.clip(((xx - x0) * dx + (yy - y0) * dy) / seg2, 0, 1)
            qx, qy = x0 + proj * dx, y0 + proj * dy
            seg_d = np.sqrt((xx - qx) ** 2 + (yy - qy) ** 2)
            seg_s = acc + proj * seg_len
            if d is None:
                d, s = seg_d, seg_s
            else:
                better = seg_d < d
                d = np.where(better, seg_d, d)
                s = np.where(better, seg_s, s)
            acc += seg_len
        return d, s

    branch_dists_arcs = [dist_and_arclen_to_polyline(p) for p in branches]
    branch_dists = [da[0] for da in branch_dists_arcs]
    branch_arcs = [da[1] for da in branch_dists_arcs]
    branch_len_cum = []
    for p in branches:
        cum = [0.0]
        for i in range(len(p) - 1):
            cum.append(cum[-1] + math.dist(p[i], p[i + 1]))
        branch_len_cum.append(cum)

    for i in range(frames):
        t = i / max(frames - 1, 1)
        reveal = ease_out_cubic(min(1.0, t / 0.55))
        open_t = ease_out_cubic(max(0.0, (t - 0.45) / 0.55))

        img = np.tile(BG, (h, w, 1))
        crack_glow = np.zeros((h, w), dtype=np.float32)
        gap_dark = np.zeros((h, w), dtype=np.float32)
        gap_glow = np.zeros((h, w), dtype=np.float32)

        for bi, p in enumerate(branches):
            total = branch_len_cum[bi][-1]
            revealed_len = total * reveal
            # crude revealed mask: only count distance where the nearest point along the
            # polyline is within revealed_len (approx via per-segment progressive reveal)
            seg_mask = np.zeros((h, w), dtype=bool)
            acc = 0.0
            for si in range(len(p) - 1):
                seg_len = math.dist(p[si], p[si + 1])
                if acc > revealed_len:
                    break
                frac = np.clip((revealed_len - acc) / max(seg_len, 1e-6), 0, 1)
                (x0, y0), (x1, y1) = p[si], p[si + 1]
                xe, ye = x0 + frac * (x1 - x0), y0 + frac * (y1 - y0)
                dx, dy = xe - x0, ye - y0
                seg2 = max(dx * dx + dy * dy, 1e-6)
                proj = np.clip(((xx - x0) * dx + (yy - y0) * dy) / seg2, 0, 1)
                qx, qy = x0 + proj * dx, y0 + proj * dy
                seg_d = np.sqrt((xx - qx) ** 2 + (yy - qy) ** 2)
                seg_mask |= seg_d < 2.5
                acc += seg_len
            crack_glow = np.maximum(crack_glow, seg_mask.astype(np.float32))
            if open_t > 0:
                gap_w = 4 + 22 * open_t
                d = branch_dists[bi]
                s = branch_arcs[bi]
                along_revealed = s <= revealed_len
                gap = np.clip(1.0 - d / gap_w, 0, 1) * along_revealed
                gap_dark = np.maximum(gap_dark, gap)
                rim = np.clip(1.0 - np.abs(d - gap_w) / 6.0, 0, 1) * along_revealed
                gap_glow = np.maximum(gap_glow, rim)

        # thin bright crack line
        crack_color = np.array([255, 240, 180], dtype=np.float32)
        img = img * (1 - crack_glow[..., None]) + crack_color * crack_glow[..., None]
        # dark chasm opening with glowing orange rim
        dark_color = np.array([8, 6, 6], dtype=np.float32)
        glow_color = np.array([255, 110, 20], dtype=np.float32)
        img = img * (1 - gap_dark[..., None]) + dark_color * gap_dark[..., None]
        img = img * (1 - gap_glow[..., None] * 0.9) + glow_color * (gap_glow[..., None] * 0.9)

        Image.fromarray(np.clip(img, 0, 255).astype(np.uint8)).save(os.path.join(out_dir, f"frame_{i:03d}.png"))
    print(f"fxgen crack: {frames} frames -> {out_dir}")


def gen_burst(out_dir, frames, w, h, ax, ay, scale, color):
    os.makedirs(out_dir, exist_ok=True)
    cx, cy = ax * w, ay * h
    max_r = 0.42 * w * scale
    n_rings = 3
    ring_delay = 0.12
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    dist = np.sqrt((xx - cx) ** 2 + (yy - cy) ** 2)
    color = np.array(color, dtype=np.float32)

    for i in range(frames):
        t = i / max(frames - 1, 1)
        img = np.tile(BG, (h, w, 1))
        # core flash, quick fade
        flash = max(0.0, 1.0 - t / 0.18) ** 2
        core_r = 0.10 * w * scale * (0.5 + 0.5 * min(1.0, t / 0.15))
        core = np.clip(1.0 - dist / max(core_r, 1e-3), 0, 1) * flash
        img = img * (1 - core[..., None]) + (color * 1.3).clip(0, 255) * core[..., None]

        for r in range(n_rings):
            rt = np.clip((t - r * ring_delay) / (1.0 - r * ring_delay), 0, 1) if t > r * ring_delay else 0
            if rt <= 0:
                continue
            radius = max_r * ease_out_cubic(rt)
            thickness = 10 + 20 * (1 - rt)
            ring = np.clip(1.0 - np.abs(dist - radius) / thickness, 0, 1) * (1.0 - rt) * 0.9
            img = img * (1 - ring[..., None]) + color * ring[..., None]

        Image.fromarray(np.clip(img, 0, 255).astype(np.uint8)).save(os.path.join(out_dir, f"frame_{i:03d}.png"))
    print(f"fxgen burst: {frames} frames -> {out_dir}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["fire", "crack", "burst"])
    ap.add_argument("out_dir")
    ap.add_argument("--frames", type=int, default=25)
    ap.add_argument("--w", type=int, default=512)
    ap.add_argument("--h", type=int, default=512)
    ap.add_argument("--anchor-x", type=float, default=0.5)
    ap.add_argument("--anchor-y", type=float, default=0.62)
    ap.add_argument("--scale", type=float, default=1.0)
    ap.add_argument("--color", default="120,200,255")
    args = ap.parse_args()

    if args.mode == "fire":
        gen_fire(args.out_dir, args.frames, args.w, args.h, args.anchor_x, args.anchor_y, args.scale)
    elif args.mode == "crack":
        gen_crack(args.out_dir, args.frames, args.w, args.h, args.anchor_x, args.anchor_y, args.scale)
    elif args.mode == "burst":
        color = tuple(int(v) for v in args.color.split(","))
        gen_burst(args.out_dir, args.frames, args.w, args.h, args.anchor_x, args.anchor_y, args.scale, color)


if __name__ == "__main__":
    main()
