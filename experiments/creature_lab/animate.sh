#!/usr/bin/env bash
# creature_lab/animate.sh
# Animate a LOW-DETAIL creature sprite into braille frames via Wan 2.2 I2V (stable-diffusion.cpp).
#
# Feed the SIMPLIFIED sprite (e.g. out/NAME_field_raw.png), NOT the high-detail creature: low detail
# downrezzes cleanly AND gives the video model less fine detail to drift on frame-to-frame.
#
# Pipeline: low-detail image -> Wan 2.2 TI2V-5B I2V -> PNG frame sequence -> braille playback (chroma-keyed)
#
# Config via env:
#   SD_CLI, MODELS, OUT        (as in generate.sh)
#   WAN        Wan 2.2 TI2V-5B gguf     (in $MODELS)
#   WAN_VAE    wan2.2_vae.safetensors   (in $MODELS)  -- 5B needs its OWN vae, not the 2.1 one
#   T5         umt5-xxl text encoder    (in $MODELS)
#   FRAMES     number of video frames   (default 33)
#   RES        square gen resolution    (default 512)
#   CHROMA     bg key color R,G,B       (default 255,255,255 — matches generate.sh's white bg)
#   CHROMA_THRESH  key tolerance        (default 50)
#
# Usage: ./animate.sh <input.png> ["motion prompt"] [name]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
SD_CLI="${SD_CLI:-$HERE/stable-diffusion.cpp/build/bin/sd-cli}"
MODELS="${MODELS:-$HERE/models}"
OUT="${OUT:-$HERE/out}"
PLAY="$HERE/../ascii_test/target/release/playframes"

WAN="${WAN:-$MODELS/Wan2.2-TI2V-5B-Q4_K_M.gguf}"
WAN_VAE="${WAN_VAE:-$MODELS/wan2.2_vae.safetensors}"
T5="${T5:-$MODELS/umt5-xxl-encoder-Q8_0.gguf}"
FRAMES="${FRAMES:-17}"   # must be 4n+1 (Wan temporal). 17=4·4+1; ping-pong ~doubles the effective loop
RES="${RES:-512}"
CHROMA="${CHROMA:-auto}"          # green screen; 'auto' samples the bg per-frame (robust to drift)
CHROMA_THRESH="${CHROMA_THRESH:-95}"

INPUT="${1:?usage: animate.sh <input.png> [\"motion prompt\"] [name]}"
MOTION="${2:-idle breathing, subtle sway, gentle motion}"
NAME="${3:-$(basename "${INPUT%.*}")}"

miss=0
for f in "$SD_CLI" "$WAN" "$WAN_VAE" "$T5" "$INPUT"; do
  [ -e "$f" ] || { echo "MISSING: $f" >&2; miss=1; }
done
[ "$miss" = 0 ] || { echo "-> see README (Animation section) for Wan model downloads" >&2; exit 1; }
[ -x "$PLAY" ] || { echo "build playframes: (cd experiments/ascii_test && cargo build --release --bin playframes)" >&2; exit 1; }

FRAMEDIR="$OUT/${NAME}_anim"
mkdir -p "$FRAMEDIR"

# Discourage the usual video-diffusion failure modes (esp. background/camera drift, which wrecks the key)
NEG="lowres, blurry, extra limbs, deformed, flickering, changing background, camera motion, zoom, pan, watermark, text"

echo ">> Wan 2.2 I2V: $INPUT -> $FRAMEDIR/frame_%03d.png  ($FRAMES frames @ ${RES}x${RES})"
echo "   NOTE: this takes a few minutes. Watch the sampling steps below — frames are only"
echo "         written at the very end (after VAE decode), so the output dir stays empty until then."
# --vae-on-cpu: Wan's VAE decode is VRAM-hungry and OOMs on 16GB at the final step; decoding on
# CPU (ample RAM) is slower but reliable. --offload-to-cpu keeps the diffusion params off the GPU too.
"$SD_CLI" -M vid_gen \
  --diffusion-model "$WAN" --vae "$WAN_VAE" --t5xxl "$T5" \
  -i "$INPUT" \
  -p "$MOTION, static locked camera, subject centered and animating in place, solid flat vivid green screen background, simple flat-color low-detail creature" \
  -n "$NEG" \
  --cfg-scale 6.0 --sampling-method euler --flow-shift 3.0 \
  -W "$RES" -H "$RES" --video-frames "$FRAMES" --diffusion-fa --offload-to-cpu --vae-on-cpu -v \
  -o "$FRAMEDIR/frame_%03d.png"

echo ">> braille playback (chroma-key $CHROMA, ping-pong loop) — q to quit"
"$PLAY" "$FRAMEDIR" --chroma "$CHROMA" --chroma-thresh "$CHROMA_THRESH" --pingpong --fps 12
