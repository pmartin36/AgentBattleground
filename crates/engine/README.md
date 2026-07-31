# Engine

A game engine that runs in your terminal. Written in Rust. It draws sprites, animation, shadows,
cameras, and UI as colored braille dots, so a terminal window behaves like a small graphics display
instead of a text screen.

![Left: the source animation in true color. Right: the same frames drawn by the engine as colored braille.](../../experiments/ascii_test/demo.gif)

Both panes above are the same terminal window. The left one is the source animation at full color,
there for comparison. The right one is what the engine draws.

## Why braille

A terminal character cell is normally one glyph in one color. Braille characters carry 8 dots in a
2×4 pattern, so the engine gets 8 addressable positions per cell instead of 1: four times the detail
of half-block art, in any terminal that can print Unicode and truecolor. It needs no GPU, no image
protocol, and no sixel support.

The catch is that the 8 dots in a cell share a single color, so the whole pipeline is designed around
that. Shape is decided dot by dot and color is averaged per cell, which keeps silhouettes crisp and
lets a sprite be recolored without deforming. Transparency is real rather than a keyed color, so
sprites layer over each other and over the background.

Everything visual goes through that one path. Creatures, panels, buttons, borders, shadows, and
effects are all dots. Only actual text is text.

## What it gives you

**Sprites and animation.** Load a PNG or GIF and draw it. Frames are sampled by elapsed time, sprites
carry position, rotation, scale, and mirroring, and tinting preserves the silhouette so a recolored
creature still looks like itself.

**Cameras and depth.** World coordinates are separate from screen dots. Pick an orthographic or
perspective camera, move it, and sprites composite back to front automatically. Depth layering is how
a crowd gets parallax: distant things smaller, dimmer, and slower.

**Layout that fits the art, not the grid.** A flexbox-style layout engine works in dots rather than
character cells, so elements can land on a half-cell boundary and stay aligned with each other.
Anchoring, stacking, gaps, grow and shrink weights, and tweened rectangles are all included.

**Scenes.** A scene is one full-screen mode: a menu, a battlefield, a settings page. The engine owns
the lifecycle and transitions between them, and a game declares its own scene list. The engine ships
no scenes of its own.

**Widgets.** Buttons with hover and press states and procedurally drawn borders. A full multi-line
text editor with wrapping, selection, system clipboard, undo, autocomplete mentions, scrollbar, and a
blinking caret.

**Audio.** Music and effects on separate volume buses, with fades, intro-then-loop regions, and mute.
If the machine has no audio device, playback silently no-ops instead of failing.

**A live inspector.** Launch the game with `--inspect` and a desktop window opens alongside the
terminal. It lists every scene, switches between them, and shows the running scene's state as
editable fields. Drag a slider and the terminal updates while the game runs. Fields opt in with a
derive macro and a couple of attributes, so exposing new state is a one-line change.

**Built for verification.** The engine can read dots back out of a rendered frame, which means tests
assert what actually reached the screen rather than what the layout math intended. Golden-frame tests,
a cell-boundary debug overlay, and asset cache hit counters come with it.

**Redraw is cheap.** Image decoding and rasterization are cached for the life of the process, so a
static asset is converted once no matter how many frames draw it.

## Getting a game running

Implement a `Scene`, list your scenes in a catalog, and hand the first one to a run loop. Logging,
panic handling, and terminal restore are one call, so a crash inside the alternate screen still
leaves a usable terminal and a readable log.

`crates/game/` in this repo is a complete example: six scenes, a battle viewer with a moving camera,
a roster carousel, and an animated title screen.

## Layout of the code

| Crate | What it is |
|---|---|
| `engine-core` | Scenes, transitions, input routing, color, logging, inspector protocol |
| `engine-render` | The braille pipeline, cameras, layout, sprites, animation, widgets |
| `engine-derive` | The derive macro that exposes state to the inspector |
| `engine-audio` | Music and sound effects |
| `inspector` | The live-editing desktop app |

Nothing here knows about the game built on top of it. The dependency only points one way: games
depend on the engine, never the reverse.

## Status

In active use by Agent Battleground, in this repository. Not published to crates.io, and the API
still moves. Rust 2021, `cargo build --workspace`.
