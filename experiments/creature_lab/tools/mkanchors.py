#!/usr/bin/env python3
"""mkanchors.py — auto-track a creature's paw/hand point across a green-screen frame
sequence, without any ML pose model (classical CV: silhouette + convex-hull extreme
points + nearest-neighbor temporal propagation from a single seeded frame-0 point).

Produces an anchors.txt in the format attach.rs expects: "idx x y angle_deg scale"
one line per frame, angle computed so a sword sprite (default: blade pointing straight
UP from a bottom-center handle) points radially outward from the body centroid through
the tracked point.

Usage: mkanchors.py <frames_dir> <out_anchors.txt> --seed x,y [--scale 0.4] [--thresh 90]
  --seed: approximate pixel coords of the hand/paw in frame 0 (eyeball it once).
"""
import argparse
import glob
import os
import cv2
import numpy as np


def corner_key(img):
    h, w = img.shape[:2]
    corners = [img[0, 0], img[0, w - 1], img[h - 1, 0], img[h - 1, w - 1]]
    return np.mean(corners, axis=0)


def foreground_mask(img, thresh):
    key = corner_key(img)
    diff = img.astype(np.float32) - key.astype(np.float32)
    dist2 = np.sum(diff * diff, axis=2)
    mask = (dist2 > thresh * thresh).astype(np.uint8) * 255
    # clean up small speckle noise
    mask = cv2.morphologyEx(mask, cv2.MORPH_OPEN, np.ones((3, 3), np.uint8))
    mask = cv2.morphologyEx(mask, cv2.MORPH_CLOSE, np.ones((5, 5), np.uint8))
    return mask


def largest_contour(mask):
    cnts, _ = cv2.findContours(mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    if not cnts:
        return None
    return max(cnts, key=cv2.contourArea)


def centroid(cnt):
    m = cv2.moments(cnt)
    if m["m00"] == 0:
        return None
    return np.array([m["m10"] / m["m00"], m["m01"] / m["m00"]])


def limb_candidate(cnt, side, band=(0.55, 0.85)):
    """Extreme point (leftmost/rightmost) within a mid-height band of the mask's bbox — the
    band excludes the top (ears/head) and bottom (feet) so it targets the arm/paw region
    regardless of whether the arm is extended enough to show up on the convex hull."""
    pts = cnt.reshape(-1, 2)
    x0, y0, w, h = cv2.boundingRect(cnt)
    y_lo, y_hi = y0 + band[0] * h, y0 + band[1] * h
    band_pts = pts[(pts[:, 1] >= y_lo) & (pts[:, 1] <= y_hi)]
    if len(band_pts) == 0:
        band_pts = pts
    if side == "left":
        return band_pts[np.argmin(band_pts[:, 0])].astype(np.float64)
    return band_pts[np.argmax(band_pts[:, 0])].astype(np.float64)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("frames_dir")
    ap.add_argument("out_anchors")
    ap.add_argument("--seed", required=True, help="x,y approx hand position in frame 0 (used only to pick left/right side)")
    ap.add_argument("--scale", type=float, default=0.4)
    ap.add_argument("--thresh", type=float, default=90)
    args = ap.parse_args()

    seed = np.array([float(v) for v in args.seed.split(",")])
    files = sorted(glob.glob(os.path.join(args.frames_dir, "*.png")) +
                    glob.glob(os.path.join(args.frames_dir, "*.jpg")))
    if not files:
        raise SystemExit(f"no frames found in {args.frames_dir}")

    # decide once, from frame 0's centroid, whether the seed is the left or right limb
    img0 = cv2.imread(files[0])
    cnt0 = largest_contour(foreground_mask(img0, args.thresh))
    c0 = centroid(cnt0)
    side = "left" if seed[0] < c0[0] else "right"

    prev_point = seed
    lines = []
    for i, f in enumerate(files):
        img = cv2.imread(f)
        mask = foreground_mask(img, args.thresh)
        cnt = largest_contour(mask)
        if cnt is None:
            lines.append(f"{i} {prev_point[0]:.1f} {prev_point[1]:.1f} 0 {args.scale}")
            continue
        c = centroid(cnt)
        if c is None:
            c = prev_point
        point = limb_candidate(cnt, side)
        prev_point = point

        dx, dy = point[0] - c[0], point[1] - c[1]
        ang = np.degrees(np.arctan2(dx, -dy))
        lines.append(f"{i} {point[0]:.1f} {point[1]:.1f} {ang:.1f} {args.scale}")

    with open(args.out_anchors, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print(f"mkanchors: wrote {len(lines)} anchors -> {args.out_anchors}")


if __name__ == "__main__":
    main()
