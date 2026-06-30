#!/usr/bin/env bash
# preview.sh <high_res.png> <battlefield.png>
# Shows both braille fidelities in the terminal — the medium that ships:
#   CREATURE VIEWER : big braille render of the high-detail image
#   BATTLEFIELD     : small braille render of the simplified (img2img-derived) image
#
# Override widths with HI_W / FIELD_W env vars.
set -euo pipefail

DR="$(cd "$(dirname "$0")" && pwd)/target/release/downrez"
[ -x "$DR" ] || { echo "build downrez first: cargo build --release --bin downrez" >&2; exit 1; }

HI="${1:?usage: preview.sh <high_res.png> <battlefield.png>}"
FIELD="${2:-$1}"   # default: same image, to compare big vs small of one source
HI_W="${HI_W:-100}"
FIELD_W="${FIELD_W:-36}"

printf '\n\033[1m=== CREATURE VIEWER  (high detail, %s cols)  %s ===\033[0m\n\n' "$HI_W" "$HI"
"$DR" "$HI" --width "$HI_W"
printf '\n\033[1m=== BATTLEFIELD      (low detail, %s cols)  %s ===\033[0m\n\n' "$FIELD_W" "$FIELD"
"$DR" "$FIELD" --width "$FIELD_W"
printf '\n'
