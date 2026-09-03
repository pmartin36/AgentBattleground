//! The `[!!]` warning badge. Per CLAUDE.md rule 4's one owner-approved
//! exception (spec:117, b3-t2 research.md): this is plain amber HUD-copy
//! TEXT, drawn via `engine_render::label`, NEVER through the braille dot
//! pipeline. `⚠` is East-Asian-Ambiguous width (terminals disagree on 1 vs 2
//! cells); a braille icon at a 2x2-cell budget degenerates (4 wide x 8 tall
//! dots is not square). Do not "fix" this into braille.
//!
use std::cell::Cell;
use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};

use super::tooltip::TOOLTIP_WIDTH_CELLS;
use super::tooltip::CARD_TEXT_COLOR;
use super::RosterManager;
use crate::diagnostics::Diagnostic;
use crate::mention::Vocabulary;
use crate::scenes::tooltip::{self, CardLayout, RowSpec};
use engine_core::color::Rgba;
use engine_render::{label, wrapped_text, Decoration, DotRect, TextAlign, TextEditor};

/// spec:117 — ASCII, always exactly 4 cells.
pub(super) const BADGE_TEXT: &str = "[!!]";
/// spec:195 — warning amber. Warning-only severity; no error tier exists.
pub(super) const WARNING_COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00);

/// The cells `draw_badge` would write for `area`, or `None` when `area`
/// cannot hold the badge (zero width/height, or fully clipped away).
/// Placement mirrors `engine_render::label`'s own conventions: left edge at
/// `area.x`, vertically centered row, width truncated to fit.
pub(super) fn badge_rect(area: Rect) -> Option<Rect> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let width = (BADGE_TEXT.chars().count() as u16).min(area.width);
    if width == 0 {
        return None;
    }
    let row = area.y + area.height / 2;
    Some(Rect::new(area.x, row, width, 1))
}

/// Draws `BADGE_TEXT` in `WARNING_COLOR` at `badge_rect(area)` when
/// `diagnostics` is non-empty. Returns the rect actually written (the b4
/// hover hit-target), or `None` when nothing was drawn. Absence (no
/// diagnostics, or no room) is decided HERE, never by the caller.
pub(super) fn draw_badge(
    buf: &mut Buffer,
    area: Rect,
    diagnostics: &[Diagnostic],
) -> Option<Rect> {
    if diagnostics.is_empty() {
        return None;
    }
    let clipped = area.intersection(buf.area);
    let rect = badge_rect(clipped)?;
    label(buf, rect, BADGE_TEXT, TextAlign::Left, warning_style());
    Some(rect)
}

/// Preferred one-cell gap between the header label and the badge, so it reads
/// as `Instructions [!!]` rather than flush against the label. The gap is
/// dropped (badge sits flush) when the header is too narrow to hold it — at
/// the 80-column floor the header is exactly `label + badge` wide (FINDING 1),
/// so a gap there would push the badge into the Edit button.
pub(super) const HEADER_BADGE_GAP_CELLS: u16 = 1;

/// The `BADGE_TEXT`-wide cell run just right of `label` inside `header`'s cell
/// rect, offset by [`HEADER_BADGE_GAP_CELLS`] when the header can hold it and
/// flush otherwise. `None` when the header cannot hold even `label` + the
/// badge flush (< 80 terminal columns — research.md b4-t1 FINDING 1), so the
/// badge is omitted rather than overwriting the header label. b4-t1.
pub(super) fn header_badge_slot(header: DotRect, label: &str) -> Option<Rect> {
    let hc = header.to_cell_rect();
    let label_w = label.chars().count() as u16;
    let badge_w = BADGE_TEXT.chars().count() as u16;
    if hc.height == 0 || hc.width < label_w + badge_w {
        return None;
    }
    // Take the gap only if it fits; otherwise sit flush at the label's end.
    let gap = if hc.width >= label_w + HEADER_BADGE_GAP_CELLS + badge_w {
        HEADER_BADGE_GAP_CELLS
    } else {
        0
    };
    Some(Rect::new(hc.x + label_w + gap, hc.y, badge_w, hc.height))
}

/// Mirrors `tooltip/mod.rs:319`'s `text_style()` — the codebase's Rgba ->
/// ratatui `Color` conversion shape.
fn warning_style() -> Style {
    Style::default().fg(Color::Rgb(WARNING_COLOR.r, WARNING_COLOR.g, WARNING_COLOR.b))
}

/// One header row.
pub(super) const HEADER_HEIGHT_CELLS: u16 = 1;
/// Cells reserved for one entry's wrapped message. 2 is exact for every
/// message the six kinds produce at the card's 32-cell interior
/// (research.md verdict #4); a longer one degrades to a tail `…`, as flavor
/// text does.
pub(super) const ENTRY_MAX_LINES: u16 = 2;
/// Blank cells above each entry — separates the header from the first entry
/// and each entry from the one before it.
pub(super) const ENTRY_GAP_CELLS: u16 = 1;

/// spec:129/220 — pluralized at the boundary: 1 -> "1 issue", else "N issues".
pub(super) fn count_header(count: usize) -> String {
    format!("{count} issue{}", if count == 1 { "" } else { "s" })
}

/// Pure row plan + geometry, delegated whole to `tooltip::layout`.
/// `rows[0]` is the count header; `rows[1..]` is one rect per diagnostic, in
/// `diagnostics` order. Total: `diagnostics.len() + 1`.
pub(super) fn layout_warning_card(anchor: DotRect, diagnostics: &[Diagnostic]) -> CardLayout {
    let mut rows = Vec::with_capacity(diagnostics.len() + 1);
    rows.push(RowSpec { height_cells: HEADER_HEIGHT_CELLS, gap_above_cells: 0 });
    for _ in diagnostics {
        rows.push(RowSpec { height_cells: ENTRY_MAX_LINES, gap_above_cells: ENTRY_GAP_CELLS });
    }
    tooltip::layout(anchor, &rows, TOOLTIP_WIDTH_CELLS)
}

/// Draws the frame + count header + one wrapped entry per diagnostic. Draws
/// NOTHING for an empty `diagnostics` — absence is decided HERE, never by
/// the caller, exactly as in `draw_badge`.
pub(super) fn render_warning_card(buf: &mut Buffer, anchor: DotRect, diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        return;
    }
    let layout = layout_warning_card(anchor, diagnostics);
    if !tooltip::draw_frame(buf, layout.card) {
        return;
    }
    label(
        buf,
        layout.rows[0].to_cell_rect(),
        &count_header(diagnostics.len()),
        TextAlign::Left,
        warning_style(),
    );
    for (diag, rect) in diagnostics.iter().zip(&layout.rows[1..]) {
        wrapped_text(buf, rect.to_cell_rect(), &diag.message, TextAlign::Left, entry_style(), true);
    }
}

/// Card body text (white), mirroring `tooltip/mod.rs:319`'s `text_style()`.
fn entry_style() -> Style {
    Style::default().fg(Color::Rgb(CARD_TEXT_COLOR.r, CARD_TEXT_COLOR.g, CARD_TEXT_COLOR.b))
}

/// Char-indexed `(line, col)` for `byte`, matching `TextEditor::set_text`'s
/// `split('\n')` (text_editor/mod.rs:211) EXACTLY — `\r` counts as a char, and a
/// trailing newline yields a final empty line. NOT `str::lines()`: it strips
/// `\r` and drops the final empty line, both of which desync cols from the
/// editor. `None` for an out-of-range or non-char-boundary offset (never
/// panics). b3-t4.
fn byte_to_pos(text: &str, byte: usize) -> Option<(usize, usize)> {
    if byte > text.len() || !text.is_char_boundary(byte) {
        return None;
    }
    let before = &text[..byte];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Some((line, before[line_start..].chars().count()))
}

/// A `Diagnostic`'s byte range -> the char-indexed, end-exclusive `(line,col)`
/// span `Decoration` takes. `None` if the range does not lie on `text`. b3-t4.
pub(super) fn span_to_line_col(
    text: &str,
    span: &Range<usize>,
) -> Option<((usize, usize), (usize, usize))> {
    let start = byte_to_pos(text, span.start)?;
    let end = byte_to_pos(text, span.end)?;
    Some((start, end))
}

/// Decorations for a diagnostic list, each paired with the index of the
/// diagnostic it came from. `decorations_for` FILTERS — `PromptTooLong` has
/// no span, and an unmappable span yields nothing — so a
/// `TextEditor::decoration_at` index (render.rs:59, "index into the
/// decoration list") does NOT index the original `&[Diagnostic]`. This type
/// carries that correspondence so no caller rebuilds it. b3-t4.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct DiagnosticDecorations {
    /// `(index into the `diagnostics` slice, its decoration)`, in input
    /// order. One entry per diagnostic with a mappable span. The ONE source
    /// of truth: pairs, never parallel vecs.
    entries: Vec<(usize, Decoration)>,
}

impl DiagnosticDecorations {
    /// The list to hand to `TextEditor::set_decorations`. Position `i` of
    /// the returned `Vec` is the diagnostic `self.diagnostic_index(i)`.
    pub(super) fn decorations(&self) -> Vec<Decoration> {
        self.entries.iter().map(|&(_, d)| d).collect()
    }

    /// Map a `TextEditor::decoration_at` result back to its index in the
    /// `diagnostics` slice `decorations_for` was given. `None` if out of
    /// range. Nothing production-side consumes per-span hover yet (b3-t4
    /// CLEANLINESS); retained so the index-desync is unrepresentable for a
    /// future consumer.
    #[allow(dead_code)]
    pub(super) fn diagnostic_index(&self, decoration_index: usize) -> Option<usize> {
        self.entries.get(decoration_index).map(|&(i, _)| i)
    }
}

/// One decoration per diagnostic with a mappable span, in input order,
/// tinted `bg`. `span: None` (PromptTooLong) and unmappable spans contribute
/// none — the returned value records which diagnostic each surviving
/// decoration came from. `text` MUST be the string the editor holds
/// (`editor.text()`) or the offsets desync. b3-t4.
pub(super) fn decorations_for(
    text: &str,
    diagnostics: &[Diagnostic],
    bg: Color,
) -> DiagnosticDecorations {
    let entries = diagnostics
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let span = span_to_line_col(text, d.span.as_ref()?)?;
            Some((i, Decoration { span, bg }))
        })
        .collect();
    DiagnosticDecorations { entries }
}

// ── b4-t2: prompt-editor badge/tint/hover-card wiring ──────────────────────

/// spec:196 — the inline bad-span tint. A dark warm red: deliberately clear
/// of BOTH editor blues, `SELECTION_BG` (0x2f,0x4f,0x6f) and
/// `MENTION_HIGHLIGHT_BG` (render.rs:30, 0x3a,0x5a,0x8a), so a bad span stays
/// distinguishable when selected AND under the mention popup's highlighted
/// row. NEVER re-declare `SELECTION_BG`'s literal game-side (spec:152) — it
/// is imported and compared.
pub(super) const DIAGNOSTIC_BG: Color = Color::Rgb(0x5f, 0x2f, 0x2f);

/// The prompt editor's diagnostics: lint cache, `[!!]` badge hit-rect, and
/// hover state (research.md b4-t2 blueprint). Lives here, not on
/// `PromptEditor` (which is already over the file-size budget).
pub(super) struct DiagnosticsState {
    vocab: Option<Vocabulary>,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) lint_runs: usize,
    pub(super) badge_hit_rect: Cell<Option<Rect>>,
    pub(super) hovered_badge: bool,
}

impl DiagnosticsState {
    pub(super) fn new(vocab: Option<Vocabulary>) -> Self {
        Self {
            vocab,
            diagnostics: Vec::new(),
            lint_runs: 0,
            badge_hit_rect: Cell::new(None),
            hovered_badge: false,
        }
    }

    /// Re-lints `text` and republishes the editor's decorations. The ONE
    /// place the editor's lint runs. `text` MUST be the string the editor
    /// holds (`editor.text()`) or the byte offsets desync (b3-t4 seam
    /// contract). No-op (and no `lint_runs` bump) when `vocab` is `None`
    /// (an off-roster creature index).
    pub(super) fn relint(&mut self, text: &str, editor: &mut TextEditor) {
        let Some(vocab) = &self.vocab else { return };
        self.diagnostics = crate::diagnostics::lint(text, vocab);
        editor.set_decorations(decorations_for(text, &self.diagnostics, DIAGNOSTIC_BG).decorations());
        self.lint_runs += 1;
    }

    /// Draws the badge into `slot` (`None` = no room) and records the
    /// hit-target. Absence stays `draw_badge`'s decision — no caller-side
    /// `is_empty()`. `slot` sits inside the popup's `Dot::Occlude` pass, so
    /// it must be cleared to a literal blank space FIRST (unconditionally):
    /// otherwise an absent badge leaves that reserved run showing the
    /// occluder's braille-blank glyph rather than "nothing here" (deviation
    /// from research.md b4-t2's pinned body — a real gap that body did not
    /// anticipate; see code-writer.md DEVIATIONS).
    pub(super) fn draw_badge_in(&self, buf: &mut Buffer, slot: Option<Rect>) {
        if let Some(rect) = slot {
            for y in rect.top()..rect.bottom() {
                for x in rect.left()..rect.right() {
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_symbol(" ");
                    }
                }
            }
        }
        self.badge_hit_rect.set(slot.and_then(|s| draw_badge(buf, s, &self.diagnostics)));
    }

    /// Hit-tests `pos` against the last-drawn badge rect.
    pub(super) fn set_hover(&mut self, pos: Position) {
        self.hovered_badge = self.badge_hit_rect.get().is_some_and(|r| r.contains(pos));
    }

    /// The hover card, anchored on the drawn badge. No-op unless hovered.
    pub(super) fn render_card(&self, buf: &mut Buffer) {
        if !self.hovered_badge {
            return;
        }
        let Some(badge) = self.badge_hit_rect.get() else { return };
        render_warning_card(buf, RosterManager::cell_rect_to_dots(badge), &self.diagnostics);
    }
}
