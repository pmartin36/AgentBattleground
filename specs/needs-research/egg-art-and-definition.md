# Needs research — Egg art & definition

> **Status: needs research.** The current egg sprites look bad. Before the hatchery UX is called finished, the eggs themselves need a better visual treatment. This is deferred out of `79-hatchery-roster-style-layout` (which fixes the layout around the egg, not the egg art) and picked up here once the layout lands.

## The problem
Live testing of the creation flow surfaced it plainly: the eggs "look BAD." The undefined-egg placeholder (the bundled `?` sprite) and the defined-egg art (the `generate_image` still, element-tinted in `tray::draw_egg`) do not read as appealing eggs at the tray/large sizes the hatchery shows them.

## Questions to answer
- Is the fix an **art-asset** problem (a better authored egg sprite / `?` placeholder, better silhouette and shading at braille resolution) or a **generation** problem (the `generate_image` prompt/pipeline producing weak egg stills)?
- Should an undefined egg read as a generic mystery egg, or already hint at its element before it is defined?
- Do defined eggs need per-element authored shells composited with the generated content, rather than a flat multiply-tint?
- What reference art (real game egg/creature-collection screens) sets the bar for "good" here? (Do the design-research pass first, per the house rule, before authoring anything.)

## Not yet in scope to decide
Whether this becomes an asset-pipeline spec, a generation-prompt spec, or both. That split is the output of the research, not an input.

## Related
- `79-hatchery-roster-style-layout` — the layout this rides behind; it explicitly defers egg art here.
- `65-hatchery`, `67-hatchery-definition-generation` — the egg lifecycle and the `generate_image` still that produces defined-egg art.
- `tray::draw_egg` — where an egg's sprite is drawn and tinted today.
