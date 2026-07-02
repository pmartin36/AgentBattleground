# FAILED_EXPERIMENTS — creature animation R&D log

Hard-won learnings from trying to generate a **lively, controlled attack animation** (a mouse sword-slash)
for an arbitrary creature, locally, on a 16 GB GPU (stable-diffusion.cpp). Recorded so we don't
re-run these dead ends. **Idle animation works great — this doc is specifically about *attack* motion.**

## The goal
Given one low-detail creature image + a free-text action, produce a lively animation for *any*
creature and *any* action ("alien on a pogostick attacking via telepathy"), locally, downrez to braille.

## What WORKS (keep)
- **Local generation** (Z-Image Turbo) — great low-detail creatures.
- **Idle animation** (Wan 2.2 TI2V-5B I2V) — subtle motion (breathe/sway/bob) is reliably coherent and alive.
- **Green-screen keying** + `downrez/playframes --chroma auto` — robust background removal (see main README).
- **Braille down-rez** — the whole render pipeline.

## What FAILED, and why (the attack-motion wall)

| # | Approach | Result | Root cause |
|---|---|---|---|
| 1 | I2V, text prompt "sword slash", 17f | **Mouse melted** mid-swing into a blur; sword vanished | Single-image I2V invents large/fast motion from one frame → loses character & prop in big per-frame jumps |
| 2 | I2V, contained prompt, 33f | Coherent, but only a **wind-up/brandish, never a strike** | Constraints that stop melting also suppress the decisive motion. Text can't specify a *novel/large* action (see research: arXiv 2510.26794) |
| 3 | FLF2V (first→last frame) | Coherent raise→strike, but **lifeless/linear**; end keyframe softer | 2-keyframe interpolation is monotonic — no anticipation possible (a windup requires reversing direction). Diffusion MSE objective is ~invariant to temporal coherence (VideoJAM, arXiv 2502.02492) |
| 4 | VACE V2V, crude synthetic control (ellipse+bar) | **Green egg + stick** — mouse identity destroyed | VACE V2V follows the control as *dense structure*; an abstract control that doesn't resemble the subject overwrites identity |
| 5 | VACE V2V, low vace-strength (0.5) | Identity back, but sword = **floppy energy streak**; pose spun around | Low strength blends identity back but weakens the control object into a wisp |
| 6 | VACE V2V, **silhouette-matched** control + windup, str 0.8–0.95 | **Stills looked great** (real sword, identity!) but in braille motion = **double sword** | Control silhouette (derived from the mouse image) baked in the mouse's *original* sword (static) *plus* the animated one → two swords |
| 7 | VACE V2V, **swordless** reference + control-sword | Double-sword gone, but sword = **faint gray wisp** | With no blade in the reference, VACE-1.3B can't *manifest* a solid new weapon from a control bar + text |

### The core tension (confirmed from both sides)
- **Sworded reference** → solid weapon, but the reference's weapon persists → **double weapon**.
- **Swordless reference** → single source, but the model can't **conjure a solid weapon** → **ghost streak**.
- Net: **VACE-1.3B cannot animate a *singular, solid* rigid prop that isn't already fixed in the frame.**

### Also confirmed
- **Text alone cannot reliably specify an arbitrary/novel action** — the field controls specific motion via
  **trajectories or reference motion**, not text (ATI, Motion Prompting, VACE, Go-with-the-Flow).
- **Stills lie.** Every failure looked fine as a still and fell apart in braille *motion*. Judge in the
  shipping medium (braille playback), across the whole arc — not isolated frames.
- **VRAM notes:** Wan 2.2 5B and Wan 2.1 VACE 1.3B both run on 16 GB with `--offload-to-cpu --vae-on-cpu`
  (contradicts a common blog claim that "Wan needs 24 GB" — that's the 14B). VACE GGUF from calcuis hit a
  5-D-tensor read bug (dropped `vace_patch_embedding`); the **safetensors** version works.

## Not yet exhausted (open threads)
- **Multi-keyframe FLF2V + playback easing** — chain neutral→windup→strike keyframes, then non-uniform
  frame timing (slow windup, fast snap) to inject "life" into linear interpolation. Blocker: reliably
  *generating* the intermediate pose keyframes.
- **Wan 2.1/2.2 VACE 14B** — bigger model might manifest a solid weapon (tight on 16 GB, slow, unproven).
- **Motion + effects model** — don't animate the weapon at all: reliable whole-creature motion
  (idle / lunge / recoil) + an **authored braille effect** (slash arc / burst / pulse) + target recoil.
  General across any creature/action; zero diffusion gamble on the hard part. **Current leading direction.**

## Recommendation
Stop grinding the literal per-creature weapon-swing. Route around it: **reliable creature motion +
authored braille attack effects.** Revisit generated weapon motion if a stronger local model lands.
