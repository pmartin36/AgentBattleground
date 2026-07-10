#!/usr/bin/env python3
"""slasheffect.py — generalized motion-trail effect anchored to a tracked weapon/limb tip.
Not sword-specific: it just draws a fading trail through the last few tip positions from
anchors.txt whenever angular velocity is high, so any effect-worthy swing (blade, claw, tail,
telepathic bolt) gets the same treatment as long as something is being tracked.

Usage: slasheffect.py <frames_dir> <anchors.txt> <out_dir> [--color R,G,B] [--trail 4] [--vel-thresh 8]
"""
import argparse
import glob
import os
from PIL import Image, ImageDraw


def load_anchors(path):
    anchors = {}
    for line in open(path):
        p = line.split()
        if len(p) < 5:
            continue
        anchors[int(p[0])] = (float(p[1]), float(p[2]), float(p[3]), float(p[4]))
    return anchors


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("frames_dir")
    ap.add_argument("anchors")
    ap.add_argument("out_dir")
    ap.add_argument("--color", default="120,240,255")
    ap.add_argument("--trail", type=int, default=4)
    ap.add_argument("--vel-thresh", type=float, default=8.0, help="deg/frame angular speed to trigger the effect")
    args = ap.parse_args()
    color = tuple(int(v) for v in args.color.split(","))

    anchors = load_anchors(args.anchors)
    files = sorted(glob.glob(os.path.join(args.frames_dir, "*.png")))
    os.makedirs(args.out_dir, exist_ok=True)

    idxs = sorted(anchors.keys())
    for i, f in enumerate(files):
        im = Image.open(f).convert("RGBA")
        overlay = Image.new("RGBA", im.size, (0, 0, 0, 0))
        d = ImageDraw.Draw(overlay)

        # angular velocity at this frame (deg/frame), to decide whether to show a trail at all
        if i > 0 and i in anchors and (i - 1) in anchors:
            ang_vel = abs(anchors[i][2] - anchors[i - 1][2])
        else:
            ang_vel = 0.0

        if ang_vel >= args.vel_thresh:
            trail_pts = [anchors[j][:2] for j in range(max(0, i - args.trail), i + 1) if j in anchors]
            n = len(trail_pts)
            for k in range(1, n):
                a0, a1 = trail_pts[k - 1], trail_pts[k]
                alpha = int(200 * (k / max(1, n - 1)))
                width = max(2, int(6 * (k / max(1, n - 1))))
                d.line([a0, a1], fill=(*color, alpha), width=width)

        im = Image.alpha_composite(im, overlay)
        im.convert("RGB").save(os.path.join(args.out_dir, os.path.basename(f)))
    print(f"slasheffect: wrote {len(files)} frames -> {args.out_dir}")


if __name__ == "__main__":
    main()
