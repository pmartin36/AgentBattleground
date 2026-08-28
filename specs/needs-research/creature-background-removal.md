# Creature background removal (needs research)

> **Status: parked pending research.** How the background is removed from generated creature art — both the still and the animation frames — so the subject composites cleanly as a braille sprite. Feeds `66-asset-generation-api`'s image and animation backends. Not a blocker for the rest of `66`; the pipeline reserves the step and this decides its method.

## Why this needs research
Z-Image and MiniMax H3 are RGB models: they cannot ingest or emit alpha, so every generated subject comes out sitting on a solid key-color screen (green, or magenta for green-family creatures). The animated silhouette changes every frame, so a single mask made from the still cannot cut later frames — separation has to happen per frame, after generation. The current path keys with a hard binary chroma cutoff (`playframes --chroma auto`, no despill), which leaves green fringing on anti-aliased edges. That fringe is partly masked when judged in braille (a braille cell averages up to 8 subpixels), so it can look fine in the shipping medium while being visibly dirty in the raw frames.

## Options to evaluate
- **Per-frame matting (rembg)** — U2Net / isnet-anime / birefnet alpha matting on each frame. Clean edges, no key color needed, but slower and adds a model dependency. rembg 2.0.76 is present in `experiments/creature_lab/.venv` (core library importable; the `[cli]` extras are missing, so drive it via the Python API, not the console script).
- **Chroma-key + despill** — keep the fast chroma cutoff but add edge despill (remove green spill from border pixels) and optional feather/erode. No new model, fixes the specific fringing.
- **Hybrid** — chroma-key for the bulk, matting only at the silhouette border.
- **Input pre-clean** — `cleanbg.py` (cosine-similarity background + shadow strip to a uniform flat color) already runs on the still before generation; evaluate whether a more uniform key color upstream reduces downstream fringing.

## Decisions needed
- Method for the **still** cutout vs. the **animation frames** (may differ).
- Whether the choice is worth a model dependency (rembg) or should stay dependency-light (chroma + despill).
- Judge on the **real braille output**, not raw PNGs alone, and confirm with a raw-frame fringe check so a medium that hides fringing doesn't hide a real regression.

## Dependencies
- Feeds `66-asset-generation-api` — supplies the background-removal step its backends reserve.
