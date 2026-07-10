#!/usr/bin/env python3
"""ikrig.py — FABRIK-driven 2D cutout limb rig, feasibility prototype.

Unlike rig_arm.py (hand-authored joint-angle timeline, synthetic drawn capsule limb —
rejected by the user for looking bolted-on and requiring per-creature manual measurement),
this tool:
  1. Takes joint positions already estimated by vision-LLM pointing (shoulder/elbow/wrist
     or hip/knee/paw — any 3-point 2-bone chain) as input, not hand-measured pixel offsets.
  2. Drives the limb via FABRIK inverse kinematics against a moving Cartesian end-effector
     target (rest -> wind -> strike), not hand-authored joint angles.
  3. Renders by cutting the ACTUAL limb pixels out of the source art (two rigid pieces,
     hinged at the joint) and rotating them into the IK-solved pose — not drawing a new
     synthetic shape — so the rendered limb keeps the original art style/texture.

Usage:
  ikrig.py <body_image> <out_frames_dir> \
      --root x,y --joint x,y --tip x,y \
      --wind x,y --strike x,y [--rest x,y] \
      [--thickness a,b,c] [--frames 25]

--root/--joint/--tip: rest-pose positions of the 3-point chain (e.g. shoulder, elbow, wrist).
--wind/--strike: Cartesian target positions for the tip (end effector) at the windup and
  strike poses. --rest defaults to --tip (arm starts at its natural rest position).
--thickness a,b,c: capsule diameter at root/joint/tip (px). Auto-estimated if omitted.
"""
import argparse
import math
import os
import cv2
import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter


def foreground_mask_cv(body_rgb, thresh=90):
    """Corner-keyed chroma foreground mask (same technique as mkanchors.py), used both for
    auto-thickness measurement and for a cheap validation check that a proposed joint point
    actually lands on the creature, not on background/smoke-fx pixels."""
    arr = np.array(body_rgb.convert("RGB"))
    h, w = arr.shape[:2]
    corners = [arr[0, 0], arr[0, w - 1], arr[h - 1, 0], arr[h - 1, w - 1]]
    key = np.mean(corners, axis=0)
    diff = arr.astype(np.float32) - key.astype(np.float32)
    dist2 = np.sum(diff * diff, axis=2)
    mask = (dist2 > thresh * thresh).astype(np.uint8) * 255
    # NOTE: no MORPH_OPEN here (mkanchors.py uses one) — opening erodes thin real anatomy
    # (wiry branch-fingers, spindly limbs) enough to disconnect it from the main silhouette,
    # and the solid-fill below then drops it entirely since it becomes its own smaller contour.
    # Confirmed on the treant: opening amputated its clawed hands outright. MORPH_CLOSE alone
    # (fills small gaps without eroding thin protrusions) plus the solid-fill handles the actual
    # problem (small internal texture/shadow holes) without this side effect.
    mask = cv2.morphologyEx(mask, cv2.MORPH_CLOSE, np.ones((5, 5), np.uint8))
    # solid-fill the largest external contour: dark interior texture (rock cracks, deep fur
    # shadow) can sit close enough to the key color to punch small internal holes in the raw
    # mask — harmless for mkanchors.py's extreme-point tracking, but a ray-cast width scan can
    # walk straight through one of those holes and report a false, too-narrow edge.
    cnts, _ = cv2.findContours(mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    if cnts:
        solid = np.zeros_like(mask)
        cv2.drawContours(solid, [max(cnts, key=cv2.contourArea)], -1, 255, thickness=-1)
        mask = solid
    return mask


def point_on_foreground(mask, pt, radius=3):
    x, y = int(round(pt[0])), int(round(pt[1]))
    h, w = mask.shape[:2]
    x0, x1 = max(0, x - radius), min(w, x + radius + 1)
    y0, y1 = max(0, y - radius), min(h, y + radius + 1)
    if x1 <= x0 or y1 <= y0:
        return False
    return mask[y0:y1, x0:x1].mean() > 127


def scan_width(mask, center, perp, max_r=260):
    """Silhouette cross-section width at `center`, scanning outward along the perpendicular
    unit vector `perp` in both directions until leaving the foreground mask."""
    h, w = mask.shape[:2]

    def edge_dist(sign):
        for r in range(1, max_r):
            x = int(round(center[0] + sign * perp[0] * r))
            y = int(round(center[1] + sign * perp[1] * r))
            if not (0 <= x < w and 0 <= y < h) or mask[y, x] < 127:
                return r - 1
        return max_r

    return edge_dist(1) + edge_dist(-1)


def auto_thickness_at(mask, bone_p0, bone_p1, t, samples=(-0.06, 0.0, 0.06)):
    """Median cross-section width near parameter t along the p0->p1 bone, sampled at a few
    nearby offsets to avoid a single unlucky slice (a claw gap, a texture crack) skewing it.
    At each offset, take the MAX width over a small fan of angles around the bone-perpendicular
    (not just the single perpendicular direction) — a round terminal mass (fist, paw) bulges
    wider off-axis than straight-on, so a single-direction scan systematically undersizes it."""
    dx, dy = bone_p1[0] - bone_p0[0], bone_p1[1] - bone_p0[1]
    length = math.hypot(dx, dy) or 1.0
    perp = (-dy / length, dx / length)

    def rotate(v, a):
        c, s = math.cos(a), math.sin(a)
        return (v[0] * c - v[1] * s, v[0] * s + v[1] * c)

    widths = []
    for s in samples:
        tt = min(1.0, max(0.0, t + s))
        c = lerp_pt(bone_p0, bone_p1, tt)
        fan = max(scan_width(mask, c, rotate(perp, da)) for da in (-0.35, 0.0, 0.35))
        widths.append(fan)
    widths.sort()
    return widths[len(widths) // 2]


def lerp(a, b, t):
    return a + (b - a) * t


def lerp_pt(p, q, t):
    return (lerp(p[0], q[0], t), lerp(p[1], q[1], t))


def ease_in(u):
    return u * u


def ease_out_cubic(u):
    return 1.0 - (1.0 - u) ** 3


def fabrik_solve(pts, lengths, target, iterations=12, tol=0.5):
    """Standard FABRIK (Aristidou & Lasenby): forward-reach to target, backward-reach to
    root, repeat until the end effector is within tol of target. pts[0] is the fixed root."""
    pts = [list(p) for p in pts]
    root = tuple(pts[0])
    total = sum(lengths)
    root_to_target = math.hypot(target[0] - root[0], target[1] - root[1])

    if root_to_target >= total:
        # unreachable: fully extend straight toward the target
        for i in range(len(pts) - 1):
            r = math.hypot(target[0] - pts[i][0], target[1] - pts[i][1]) or 1e-6
            lam = lengths[i] / r
            pts[i + 1][0] = pts[i][0] + (target[0] - pts[i][0]) * lam
            pts[i + 1][1] = pts[i][1] + (target[1] - pts[i][1]) * lam
        return pts

    n = len(pts)
    for _ in range(iterations):
        pts[-1] = [target[0], target[1]]
        for i in range(n - 2, -1, -1):
            r = math.hypot(pts[i + 1][0] - pts[i][0], pts[i + 1][1] - pts[i][1]) or 1e-6
            lam = lengths[i] / r
            pts[i][0] = pts[i + 1][0] + (pts[i][0] - pts[i + 1][0]) * lam
            pts[i][1] = pts[i + 1][1] + (pts[i][1] - pts[i + 1][1]) * lam
        pts[0] = list(root)
        for i in range(n - 1):
            r = math.hypot(pts[i + 1][0] - pts[i][0], pts[i + 1][1] - pts[i][1]) or 1e-6
            lam = lengths[i] / r
            pts[i + 1][0] = pts[i][0] + (pts[i + 1][0] - pts[i][0]) * lam
            pts[i + 1][1] = pts[i][1] + (pts[i + 1][1] - pts[i][1]) * lam
        if math.hypot(pts[-1][0] - target[0], pts[-1][1] - target[1]) < tol:
            break
    return pts


def capsule_mask(size, p0, p1, th0, th1, feather=3):
    """Filled tapered-capsule alpha mask from p0 (diameter th0) to p1 (diameter th1)."""
    mask = Image.new("L", size, 0)
    d = ImageDraw.Draw(mask)
    n = 16
    dx, dy = p1[0] - p0[0], p1[1] - p0[1]
    length = math.hypot(dx, dy) or 1.0
    nx, ny = -dy / length, dx / length
    top, bot = [], []
    for i in range(n + 1):
        t = i / n
        cx, cy = lerp(p0[0], p1[0], t), lerp(p0[1], p1[1], t)
        r = lerp(th0, th1, t) / 2.0
        top.append((cx + nx * r, cy + ny * r))
        bot.append((cx - nx * r, cy - ny * r))
    d.polygon(top + bot[::-1], fill=255)
    d.ellipse([p0[0] - th0 / 2, p0[1] - th0 / 2, p0[0] + th0 / 2, p0[1] + th0 / 2], fill=255)
    d.ellipse([p1[0] - th1 / 2, p1[1] - th1 / 2, p1[0] + th1 / 2, p1[1] + th1 / 2], fill=255)
    if feather:
        mask = mask.filter(ImageFilter.GaussianBlur(feather))
    return mask


def conform_to_silhouette(roi_mask, fg_mask_np, bone_p0=None, bone_p1=None, feather=0):
    """Intersect a synthetic capsule ROI with the creature's own true silhouette, so the cutout
    hugs the actual limb shape (claws, branches, spikes — anything that doesn't taper smoothly)
    instead of being bounded by an idealized capsule that either clips off real protrusions or
    leaves a sliver of un-erased original art outside its edge. `roi_mask` should already be
    generously wider than the measured thickness so real anatomy isn't clipped before the
    intersection; the ROI's job is just to localize which part of the silhouette is this limb.
    feather=0 by default: a thin branch/claw/spike can be only a few px wide, and even a small
    (2px) blur here softens a meaningful fraction of its whole cross-section — against a
    high-contrast background this reads as the piece being washed-out/translucent, not just a
    slightly soft edge. Confirmed on the treant's forearm before this default was set to 0.

    If bone_p0/bone_p1 are given, keeps only the connected component that actually touches the
    bone's own centerline, dropping any other component entirely — a small ambient shadow/
    highlight under a fist or claw is dark enough to pass the background-distance threshold on
    its own, and without this it survives the intersection as a same-looking, but disconnected,
    fragment that swings independently of the real limb and reads as a stray thread. Confirmed
    on the golem's fist (a shadow sliver) after the treant's branch fix exposed the same root
    cause (foreground-vs-shadow ambiguity) at a different scale."""
    fg_img = Image.fromarray(fg_mask_np)
    conformed = ImageChops.multiply(roi_mask.convert("L"), fg_img.convert("L"))
    if bone_p0 is not None and bone_p1 is not None:
        arr = (np.array(conformed) > 127).astype(np.uint8)
        n, labels = cv2.connectedComponents(arr)
        wanted = set()
        for t in (0.0, 0.15, 0.3, 0.5, 0.7, 0.85, 1.0):
            c = lerp_pt(bone_p0, bone_p1, t)
            cx, cy = int(round(c[0])), int(round(c[1]))
            if 0 <= cy < labels.shape[0] and 0 <= cx < labels.shape[1]:
                lbl = labels[cy, cx]
                if lbl != 0:
                    wanted.add(lbl)
        if wanted:
            keep = np.isin(labels, list(wanted)).astype(np.uint8) * 255
            conformed = Image.fromarray(keep)
    if feather:
        conformed = conformed.filter(ImageFilter.GaussianBlur(feather))
    return conformed


def cutout_piece(body_rgba, mask, pad=24):
    """Crop the masked region (with padding) out of body_rgba as its own RGBA piece.
    Returns (piece_image, crop_origin) so pivot points can be translated to local coords."""
    bbox = mask.getbbox()
    if bbox is None:
        raise ValueError("empty mask")
    x0, y0, x1, y1 = bbox
    x0, y0 = max(0, x0 - pad), max(0, y0 - pad)
    x1, y1 = min(mask.width, x1 + pad), min(mask.height, y1 + pad)
    piece = body_rgba.crop((x0, y0, x1, y1)).copy()
    local_mask = mask.crop((x0, y0, x1, y1))
    piece.putalpha(local_mask)
    return piece, (x0, y0)


def affine_rotate(piece, angle_rad, pivot_local, pivot_dest, canvas_size):
    """Paste `piece` onto a canvas of canvas_size, rotated by angle_rad about pivot_local
    (in piece-local coords) and re-anchored so that pivot lands at pivot_dest (canvas coords).
    Built as an explicit inverse affine map to avoid any rotate()-direction ambiguity."""
    c, s = math.cos(angle_rad), math.sin(angle_rad)
    px, py = pivot_local
    qx, qy = pivot_dest
    # dest = R(angle) * (src - pivot_local) + pivot_dest  =>  src = R(-angle)*(dest-pivot_dest)+pivot_local
    a, b = c, s
    d, e = -s, c
    cc = -(a * qx + b * qy) + px
    ff = -(d * qx + e * qy) + py
    out = piece.transform(canvas_size, Image.AFFINE, (a, b, cc, d, e, ff), resample=Image.BICUBIC)
    return out


def parse_pt(s):
    x, y = s.split(",")
    return (float(x), float(y))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("body_image")
    ap.add_argument("out_frames_dir")
    ap.add_argument("--root", type=parse_pt, required=True)
    ap.add_argument("--joint", type=parse_pt, required=True)
    ap.add_argument("--tip", type=parse_pt, required=True)
    ap.add_argument("--wind", type=parse_pt, required=True)
    ap.add_argument("--strike", type=parse_pt, required=True)
    ap.add_argument("--rest", type=parse_pt, default=None)
    ap.add_argument("--thickness", default=None,
                     help="th_root,th_joint,th_tip (px). Omit to auto-measure from the "
                          "creature's own silhouette (recommended — bone-length heuristics "
                          "get chunky/irregular creatures badly wrong).")
    ap.add_argument("--frames", type=int, default=25)
    ap.add_argument("--wind-frac", type=float, default=0.4)
    ap.add_argument("--no-validate", action="store_true",
                     help="skip the pre-render sanity check that root/joint/tip land on the "
                          "creature's own silhouette, not background/fx pixels")
    args = ap.parse_args()

    root, joint, tip = args.root, args.joint, args.tip
    rest = args.rest or tip
    w1 = math.hypot(joint[0] - root[0], joint[1] - root[1])
    w2 = math.hypot(tip[0] - joint[0], tip[1] - joint[1])

    body = Image.open(args.body_image).convert("RGBA")
    W, H = body.size
    fg = foreground_mask_cv(body)

    if not args.no_validate:
        bad = [name for name, p in (("root", root), ("joint", joint), ("tip", tip))
               if not point_on_foreground(fg, p)]
        if bad:
            print(f"ikrig: VALIDATION FAILED — {', '.join(bad)} not on the creature's "
                  f"silhouette (background/fx pixels). Re-check the joint coordinates. "
                  f"Pass --no-validate to force through anyway.")
            raise SystemExit(1)
        by0 = np.where(fg > 127)[0]
        if len(by0):
            top_y, bot_y = by0.min(), by0.max()
            span = max(1, bot_y - top_y)
            if (root[1] - top_y) / span < 0.15:
                print(f"ikrig: WARNING — root y={root[1]:.0f} is in the top 15% of the "
                      f"creature's silhouette ({top_y:.0f}-{bot_y:.0f}); shoulders/hips are "
                      f"rarely this high, double-check it isn't landing on the head/neck.")

    if args.thickness:
        th_root, th_joint, th_tip = (float(v) for v in args.thickness.split(","))
    else:
        # measured slightly inset from the raw endpoints, not exactly at them: t=0 (root) sits
        # right at the socket where the limb blends into the torso/hip mass, so a scan there
        # measures "limb + body" width, not limb width — and t=1 (tip) on a round terminal mass
        # (fist, paw) is past the widest point of the bulge. Both got confirmed wrong on the
        # golem/wolf test images before this inset was added.
        # inset by a minimum ABSOLUTE pixel distance, not just a fraction of this bone's own
        # length — a short bone (e.g. a stubby quadruped upper leg mostly hidden in body fur)
        # would otherwise barely clear the merge zone and still measure body-mass width.
        root_inset_t = min(0.4, max(0.15, 18.0 / max(w1, 1.0)))
        tip_inset_t = min(0.4, max(0.15, 18.0 / max(w2, 1.0)))
        th_root = auto_thickness_at(fg, root, joint, root_inset_t) or w1 * 0.62
        th_joint = auto_thickness_at(fg, root, joint, 1.0) or w1 * 0.42
        th_tip = auto_thickness_at(fg, joint, tip, 1.0 - tip_inset_t) or w2 * 0.55
        # sanity clamp: the socket/terminal end is rarely more than ~2x the mid-limb width for
        # any creature build seen so far — without this, a root/tip measurement that's still
        # inside a merge zone or a nearby unrelated body part (this genuinely happened on the
        # wolf's very short upper leg bone above) silently produces an oversized piece that
        # erases and drags in a chunk of the torso.
        th_root = min(th_root, th_joint * 2.0, w1 * 0.9)
        th_tip = min(th_tip, th_joint * 2.0, w2 * 0.9)
        th_joint = min(th_joint, w1 * 0.9, w2 * 0.9)
        print(f"ikrig: auto-measured thickness root={th_root:.0f} joint={th_joint:.0f} tip={th_tip:.0f}")

    # The capsule is a region-of-interest, not the final cutout shape: build it generously wider
    # (1.7x) than the measured thickness, then clip to the creature's own true silhouette. This
    # is what actually fixes the treant's floating-twig bug — a smooth capsule taper doesn't
    # conform to claws/branches/spikes, so it either clips real protrusions off (leaving a
    # sliver of un-erased original art behind, which reads as severed debris) or leaves empty
    # capsule padding beyond the real silhouette (harmless, just wasted piece area).
    roi_upper = capsule_mask((W, H), root, joint, th_root * 1.7, th_joint * 1.7, feather=0)
    roi_lower = capsule_mask((W, H), joint, tip, th_joint * 1.7, th_tip * 1.7, feather=0)
    mask_upper = conform_to_silhouette(roi_upper, fg, root, joint)
    mask_lower = conform_to_silhouette(roi_lower, fg, joint, tip)

    upper_piece, upper_origin = cutout_piece(body, mask_upper)
    lower_piece, lower_origin = cutout_piece(body, mask_lower)
    upper_pivot_local = (root[0] - upper_origin[0], root[1] - upper_origin[1])
    lower_pivot_local = (joint[0] - lower_origin[0], joint[1] - lower_origin[1])

    union_mask = Image.new("L", (W, H), 0)
    union_mask.paste(mask_upper, (0, 0), mask_upper)
    roi_lower_bigger = capsule_mask((W, H), joint, tip, th_joint * 1.9, th_tip * 1.9, feather=0)
    lower_bigger = conform_to_silhouette(roi_lower_bigger, fg, joint, tip)
    union_mask = Image.eval(union_mask, lambda v: v)
    for mm in (mask_upper, lower_bigger):
        union_mask = Image.composite(Image.new("L", (W, H), 255), union_mask, mm)
    # a tight feather, not a wide one: the fill now matches the true chroma-key color exactly
    # (see below), so a hard edge is invisible against the flat screen — a wide blur instead
    # creates a translucent halo that matches neither the background nor the piece cleanly,
    # which is what actually read as a visible "shoulder separation" seam.
    feathered_union = union_mask.filter(ImageFilter.GaussianBlur(1.5))

    # flat chroma-key green screen: the corners are always pure background, and sampling
    # near the erase box (as rig_arm.py does for photo-like art) picks up dark body/shadow
    # edge pixels here, muddying the fill and leaving a visible smear where it doesn't match
    # the actual bright screen color.
    corner_px = [body.getpixel((5, 5))[:3], body.getpixel((W - 5, 5))[:3],
                 body.getpixel((5, H - 5))[:3], body.getpixel((W - 5, H - 5))[:3]]
    local_bg = tuple(sum(c) // len(corner_px) for c in zip(*corner_px)) + (255,)

    body_erased = body.copy()
    bg_layer = Image.new("RGBA", (W, H), local_bg)
    body_erased = Image.composite(bg_layer, body_erased, feathered_union)

    upper_rest_angle = math.atan2(joint[1] - root[1], joint[0] - root[0])
    lower_rest_angle = math.atan2(tip[1] - joint[1], tip[0] - joint[0])

    os.makedirs(args.out_frames_dir, exist_ok=True)

    n = args.frames
    chain = [list(root), list(joint), list(tip)]
    for f in range(n):
        t = f / (n - 1) if n > 1 else 0.0
        if t < args.wind_frac:
            # decelerate INTO the wound-up anticipation pose (a held gather-power beat)
            u = ease_out_cubic(t / args.wind_frac)
            target = lerp_pt(rest, args.wind, u)
        else:
            # accelerate hard INTO the strike (a snap arriving at speed, not a pendulum easing
            # to a stop right as it should be landing) — this was backwards before and is what
            # read as "pacing was odd."
            uu = (t - args.wind_frac) / (1.0 - args.wind_frac)
            u = uu * uu * uu
            target = lerp_pt(args.wind, args.strike, u)

        chain = fabrik_solve(chain, [w1, w2], target)
        new_joint, new_tip = chain[1], chain[2]

        upper_angle = math.atan2(new_joint[1] - root[1], new_joint[0] - root[0])
        lower_angle = math.atan2(new_tip[1] - new_joint[1], new_tip[0] - new_joint[0])

        frame = body_erased.copy()
        up = affine_rotate(upper_piece, upper_angle - upper_rest_angle, upper_pivot_local, root, (W, H))
        frame = Image.alpha_composite(frame, up)
        lo = affine_rotate(lower_piece, lower_angle - lower_rest_angle, lower_pivot_local, new_joint, (W, H))
        frame = Image.alpha_composite(frame, lo)

        frame.convert("RGB").save(os.path.join(args.out_frames_dir, f"frame_{f:03d}.png"))

    print(f"ikrig: wrote {n} frames -> {args.out_frames_dir}")
    print(f"  bone lengths: upper={w1:.1f} lower={w2:.1f}, thickness=({th_root:.0f},{th_joint:.0f},{th_tip:.0f})")


if __name__ == "__main__":
    main()
