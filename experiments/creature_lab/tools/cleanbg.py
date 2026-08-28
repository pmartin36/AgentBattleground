#!/usr/bin/env python3
"""cleanbg.py — strip a chroma-key background AND any shadow blended into it, replacing both
with a perfectly uniform flat color.

Why not just threshold distance from the key color (the technique already used in ikrig.py's
foreground_mask_cv / mkanchors.py's foreground_mask): a shadow is a DARKENED version of the same
background color, which sits far from the key color in plain Euclidean RGB distance (darkening
moves all three channels down together) even though it's visually still "background." That
technique correctly finds character silhouettes but leaves shadows classified as foreground.

This uses a color-DIRECTION test instead of a color-DISTANCE test: cosine similarity between each
pixel's RGB vector and the background color's RGB vector. A shadow is background scaled down in
magnitude (same direction, shorter vector) so its cosine similarity to the background stays high
regardless of how dark it is. A character's own shading (rock grays/browns, fur tones, etc.) points
in a genuinely different color direction, so its similarity is much lower. Verified on real pixel
samples from the golem: shadow ~0.99 similarity, character dark-rock shading ~0.90 — a clean gap.

Usage: cleanbg.py <in.png> <out.png> [--similarity-thresh 0.97] [--feather 1]
"""
import argparse
import numpy as np
from PIL import Image, ImageFilter


def clean_background(img_rgb, sim_thresh=0.97, feather=1, min_ratio=0.12, max_ratio=1.05):
    arr = np.array(img_rgb.convert("RGB")).astype(np.float64)
    h, w = arr.shape[:2]
    corners = np.array([arr[0, 0], arr[0, w - 1], arr[h - 1, 0], arr[h - 1, w - 1]])
    bg = corners.mean(axis=0)
    bg_mag = np.linalg.norm(bg) + 1e-6
    bg_norm = bg / bg_mag

    # Cosine similarity (color DIRECTION vs the background) alone isn't enough: normalizing by
    # magnitude is numerically unstable for near-black pixels (dividing by ~0 amplifies tiny
    # per-channel noise into essentially random "directions"), which sporadically pushes black
    # outline/crack linework over the similarity threshold — confirmed as the cause of visible
    # stipple noise along outlines, worse on bold flat-vector art (thick pure-black outlines, pure-
    # white highlights) than on painterly-shaded art. Fix: a real shadow is background PARTIALLY
    # darkened — never near-total black (that's ink/linework) and never brighter than the
    # background (that's a highlight). Gate on magnitude ratio to the background's own magnitude,
    # not just direction — this excludes both failure modes by construction, not by re-tuning a
    # single threshold. Verified: white highlights (ratio ~1.7+) and black outlines (ratio ~0.05-0.1)
    # both fall outside [0.12, 1.05]; real shadow samples (ratio ~0.3-0.7) fall inside it.
    mag = np.linalg.norm(arr, axis=2)
    ratio = mag / bg_mag
    unit = arr / (mag[..., None] + 1e-6)
    cos_sim = unit @ bg_norm

    is_bg_or_shadow = (cos_sim > sim_thresh) & (ratio > min_ratio) & (ratio < max_ratio)

    mask = (is_bg_or_shadow.astype(np.uint8)) * 255
    mask_img = Image.fromarray(mask, mode="L")
    if feather:
        mask_img = mask_img.filter(ImageFilter.GaussianBlur(feather))

    out = np.array(img_rgb.convert("RGB")).astype(np.uint8)
    bg_flat = np.full_like(out, bg.astype(np.uint8))
    mask_f = np.array(mask_img).astype(np.float64)[..., None] / 255.0
    blended = (out.astype(np.float64) * (1 - mask_f) + bg_flat.astype(np.float64) * mask_f).astype(np.uint8)
    return Image.fromarray(blended), mask_img


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("input")
    ap.add_argument("output")
    ap.add_argument("--similarity-thresh", type=float, default=0.97)
    ap.add_argument("--feather", type=float, default=1)
    ap.add_argument("--save-mask", default=None, help="optional path to also save the mask for inspection")
    args = ap.parse_args()

    img = Image.open(args.input)
    cleaned, mask = clean_background(img, args.similarity_thresh, args.feather)
    cleaned.save(args.output)
    if args.save_mask:
        mask.save(args.save_mask)
    print(f"cleanbg: wrote {args.output}")


if __name__ == "__main__":
    main()
