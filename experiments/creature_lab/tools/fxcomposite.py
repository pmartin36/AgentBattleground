#!/usr/bin/env python3
"""fxcomposite.py — overlay a green-screen fx clip (from fxgen.py) on top of a base creature clip.
Keys the fx clip's green background out (corner-sampled, same convention as attach.rs/playframes.rs)
and pastes its non-background pixels onto the base frame. Frame counts may differ — the shorter one
is looped/held to match the longer one's length.

Usage: fxcomposite.py <base_frames_dir> <fx_frames_dir> <out_dir> [--behind]
  --behind: composite the fx UNDER the creature (painted first, creature on top) instead of on top.
"""
import argparse
import glob
import os
import numpy as np
from PIL import Image


def load_frames(d):
    files = sorted(glob.glob(os.path.join(d, "*.png")))
    return [np.array(Image.open(f).convert("RGB"), dtype=np.uint8) for f in files]


def corner_key(img):
    h, w = img.shape[:2]
    c = [img[0, 0], img[0, w - 1], img[h - 1, 0], img[h - 1, w - 1]]
    return np.mean(c, axis=0)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("base_dir")
    ap.add_argument("fx_dir")
    ap.add_argument("out_dir")
    ap.add_argument("--thresh", type=float, default=60)
    ap.add_argument("--behind", action="store_true")
    args = ap.parse_args()

    base = load_frames(args.base_dir)
    fx = load_frames(args.fx_dir)
    n = max(len(base), len(fx))
    os.makedirs(args.out_dir, exist_ok=True)

    for i in range(n):
        b = base[i % len(base)].astype(np.float32)
        f = fx[i % len(fx)].astype(np.float32)
        key = corner_key(f)
        d2 = np.sum((f - key) ** 2, axis=2)
        fx_mask = (d2 > args.thresh ** 2).astype(np.float32)[..., None]
        if args.behind:
            bg_key = corner_key(b)
            base_bg_mask = (np.sum((b - bg_key) ** 2, axis=2) < args.thresh ** 2).astype(np.float32)[..., None]
            out = f * fx_mask * base_bg_mask + b * (1 - fx_mask * base_bg_mask)
        else:
            out = f * fx_mask + b * (1 - fx_mask)
        Image.fromarray(np.clip(out, 0, 255).astype(np.uint8)).save(
            os.path.join(args.out_dir, f"frame_{i:03d}.png"))
    print(f"fxcomposite: {n} frames -> {args.out_dir}")


if __name__ == "__main__":
    main()
