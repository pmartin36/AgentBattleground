//! Mad-lib paragraph model: pure run-based greedy word-wrap over a mixed
//! literal/blank run stream, plus a paint step that draws the wrapped
//! glyphs, a braille-dot underline beneath each blank's words (growing past
//! a fixed floor as its value grows, continuing under every row a wrapped
//! blank occupies), and a `Modifier::REVERSED` caret cell reusing a
//! `TextEditor`'s own blink phase. `wrap` is pure (no `Buffer`) so layout is
//! unit-testable independent of painting; `render` calls it and paints.
//! Shared by the inline edit surface (an active `Caret`) and the read-only
//! defined-egg detail (`caret = None`, all-`Literal` runs).
//!
//! Wrapping treats the whole run stream as a single word-wrapped text: a
//! maximal span of non-whitespace glyphs forms one "atom" that is never
//! split for width reasons, and two adjacent runs with no whitespace
//! between them (e.g. a blank value directly followed by trailing
//! punctuation) render glued together with no inserted space, exactly like
//! the rest of the paragraph's own words.
#![allow(dead_code)] // consumed by the inline edit surface and the read-only defined-egg detail renderer

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use engine_core::color::Rgba;
use engine_render::DotRect;

/// Minimum underline width, in cells, an empty or short blank still shows.
pub(crate) const MIN_UNDERLINE_CELLS: u16 = 8;
/// Dot-rows thick the underline decoration is painted.
pub(crate) const UNDERLINE_THICKNESS_DOTS: i32 = 2;
/// Underline color (matches the deleted modal's `SLOT_UNDERLINE_COLOR`).
pub(crate) const UNDERLINE_COLOR: Rgba = Rgba::rgb(0x88, 0x88, 0x88);
/// Literal/blank glyph text color.
pub(crate) const TEXT_COLOR: Rgba = Rgba::rgb(0xe0, 0xe0, 0xe0);
/// Cells of whitespace placed between two adjacent words.
pub(crate) const SPACE_CELLS: u16 = 1;

/// One piece of the paragraph's run stream: fixed literal text, or a named
/// blank carrying its current (possibly empty or multi-word) value.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ParaRun<'a> {
    Literal(&'a str),
    Blank(&'a str),
}

/// The active blank + cursor position for edit mode; `None` for read-only
/// rendering. `cursor` is a char offset into that blank's value;
/// `visible` is the caller's own `TextEditor::caret_visible()` for this
/// frame (this module reuses the blink TIMING, not the editor itself).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Caret {
    pub blank_ordinal: usize,
    pub cursor: usize,
    pub visible: bool,
}

/// A placed run of contiguous cells on one wrapped text row, returned by
/// [`wrap`]. `blank` is `Some((ordinal, char_start))` for a piece that
/// belongs to a blank (`char_start` is the char offset within that blank's
/// value at which this piece begins, used for caret mapping); `None` for a
/// literal-text piece.
#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub row: u16,
    pub col: u16,
    pub cells: u16,
    pub text: String,
    pub underline: bool,
    pub blank: Option<(usize, usize)>,
}

/// An intermediate placeable unit before row/column assignment: either a
/// glyph atom (a maximal non-whitespace span from one run) or a
/// pure-underline placeholder (empty `text`, used for floor padding and the
/// empty-blank case). `gap_before` records whether whitespace separated
/// this atom from the previous one, so a blank glued to adjacent literal
/// text (no whitespace at the run boundary) renders with no inserted space.
struct Atom {
    blank: Option<(usize, usize)>,
    text: String,
    gap_before: bool,
    forced_cells: Option<u16>,
}

impl Atom {
    fn width(&self) -> u16 {
        self.forced_cells.unwrap_or(self.text.chars().count() as u16)
    }
}

/// Splits `text` into maximal non-whitespace spans, appending each as an
/// [`Atom`] tagged with `ordinal` (`None` for literal text). `pending_gap`
/// and `seen_any` thread whitespace/first-atom state across calls so a run
/// boundary with no whitespace on either side glues its atoms together
/// (`gap_before = false`).
fn push_words(
    text: &str,
    ordinal: Option<usize>,
    atoms: &mut Vec<Atom>,
    pending_gap: &mut bool,
    seen_any: &mut bool,
) {
    let mut start: Option<usize> = None;
    let mut buf = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                atoms.push(Atom {
                    blank: ordinal.map(|o| (o, s)),
                    text: std::mem::take(&mut buf),
                    gap_before: *pending_gap && *seen_any,
                    forced_cells: None,
                });
                *seen_any = true;
                *pending_gap = false;
            }
            *pending_gap = true;
        } else {
            if start.is_none() {
                start = Some(idx);
            }
            buf.push(ch);
        }
    }
    if let Some(s) = start.take() {
        atoms.push(Atom {
            blank: ordinal.map(|o| (o, s)),
            text: buf,
            gap_before: *pending_gap && *seen_any,
            forced_cells: None,
        });
        *seen_any = true;
        *pending_gap = false;
    }
}

/// Tokenizes `runs` into [`Atom`]s in order. A blank whose value is empty
/// (or all whitespace) still gets a zero-width placeholder atom at its
/// position, so the floor pass below has somewhere to grow its underline
/// from.
fn tokenize<'a>(runs: &[ParaRun<'a>]) -> Vec<Atom> {
    let mut atoms = Vec::new();
    let mut pending_gap = false;
    let mut seen_any = false;
    let mut blank_ordinal = 0usize;

    for run in runs {
        match run {
            ParaRun::Literal(text) => {
                push_words(text, None, &mut atoms, &mut pending_gap, &mut seen_any);
            }
            ParaRun::Blank(text) => {
                let ord = blank_ordinal;
                blank_ordinal += 1;
                let before = atoms.len();
                push_words(text, Some(ord), &mut atoms, &mut pending_gap, &mut seen_any);
                if atoms.len() == before {
                    atoms.push(Atom {
                        blank: Some((ord, 0)),
                        text: String::new(),
                        gap_before: pending_gap && seen_any,
                        forced_cells: None,
                    });
                    seen_any = true;
                    pending_gap = false;
                }
            }
        }
    }
    atoms
}

/// Grows each blank's underline to `max(MIN_UNDERLINE_CELLS, its own char
/// count, caret.cursor + 1 when the caret targets it)`: an empty/all-ws
/// blank's sole placeholder atom is resized in place; a non-empty blank
/// short of the target gets a trailing pure-underline placeholder atom
/// inserted right after its own last glyph atom.
fn apply_underline_floor(atoms: Vec<Atom>, caret: Option<&Caret>) -> Vec<Atom> {
    let mut real_chars: HashMap<usize, usize> = HashMap::new();
    let mut last_idx: HashMap<usize, usize> = HashMap::new();
    for (i, atom) in atoms.iter().enumerate() {
        if let Some((ord, _)) = atom.blank {
            *real_chars.entry(ord).or_insert(0) += atom.text.chars().count();
            last_idx.insert(ord, i);
        }
    }

    let mut out = Vec::with_capacity(atoms.len() + real_chars.len());
    for (i, atom) in atoms.into_iter().enumerate() {
        let Some((ord, _)) = atom.blank else {
            out.push(atom);
            continue;
        };
        if last_idx.get(&ord) != Some(&i) {
            out.push(atom);
            continue;
        }

        let chars = real_chars[&ord];
        let mut target = chars.max(MIN_UNDERLINE_CELLS as usize);
        if let Some(c) = caret {
            if c.blank_ordinal == ord {
                target = target.max(c.cursor + 1);
            }
        }
        let pad = target.saturating_sub(chars);

        if atom.text.is_empty() {
            let mut atom = atom;
            atom.forced_cells = Some(target as u16);
            out.push(atom);
        } else {
            out.push(atom);
            if pad > 0 {
                out.push(Atom {
                    blank: Some((ord, chars)),
                    text: String::new(),
                    gap_before: false,
                    forced_cells: Some(pad as u16),
                });
            }
        }
    }
    out
}

/// Pure greedy word-wrap over `runs`: places words left-to-right separated
/// by [`SPACE_CELLS`], breaking only at whitespace (never mid-word) when the
/// next word would exceed `width`. Blank text wraps identically to literal
/// text. An empty/short blank still emits underline-only placeholder cells
/// up to [`MIN_UNDERLINE_CELLS`]; when `caret` targets a blank, at least
/// `cursor + 1` underline cells exist for it. Returns an empty `Vec` for
/// `width == 0` or empty `runs`; never panics.
pub(crate) fn wrap(runs: &[ParaRun], width: u16, caret: Option<&Caret>) -> Vec<Piece> {
    if width == 0 || runs.is_empty() {
        return Vec::new();
    }

    let atoms = tokenize(runs);
    let atoms = apply_underline_floor(atoms, caret);

    let mut pieces = Vec::with_capacity(atoms.len());
    let mut row: u16 = 0;
    let mut col: u16 = 0;
    let mut row_has_content = false;

    for atom in &atoms {
        let cell_width = atom.width();
        if cell_width == 0 {
            continue;
        }
        let gap = if atom.gap_before && row_has_content { SPACE_CELLS } else { 0 };
        if row_has_content && col.saturating_add(gap).saturating_add(cell_width) > width {
            row += 1;
            col = 0;
        } else {
            col += gap;
        }

        pieces.push(Piece {
            row,
            col,
            cells: cell_width,
            text: atom.text.clone(),
            underline: atom.blank.is_some(),
            blank: atom.blank,
        });
        col += cell_width;
        row_has_content = true;
    }

    pieces
}

/// Finds the cell the caret block sits on: the first piece belonging to
/// `caret.blank_ordinal` whose char range `[start, start + cells]`
/// (inclusive, so the end-of-text position always has a home cell) contains
/// `caret.cursor`.
fn caret_cell(pieces: &[Piece], area: Rect, caret: &Caret) -> Option<(u16, u16)> {
    for p in pieces {
        let Some((ord, start)) = p.blank else { continue };
        if ord != caret.blank_ordinal {
            continue;
        }
        let end = start + p.cells as usize;
        if caret.cursor >= start && caret.cursor <= end {
            let col = area.x.saturating_add(p.col).saturating_add((caret.cursor - start) as u16);
            let glyph_row = area.y.saturating_add(p.row.saturating_mul(2));
            return Some((col, glyph_row));
        }
    }
    None
}

/// Paints a lit-dot underline `UNDERLINE_THICKNESS_DOTS` dot-rows tall,
/// spanning `cells` cells starting at cell `(col, row)`. `col`/`row` are
/// already whole cells (no sub-cell remainder to floor), so the target
/// converts to dots directly through the shared
/// `post_battle::columns::blit_dots` sub-cell placer (CLAUDE.md rule 5).
fn draw_underline(buf: &mut Buffer, col: u16, row: u16, cells: u16) {
    let target = DotRect { x: col as i32 * 2, y: row as i32 * 4, w: cells as i32 * 2, h: UNDERLINE_THICKNESS_DOTS };
    let mut local = engine_render::dots::DotBuffer::new(target.w as usize, target.h as usize);
    for y in 0..target.h as usize {
        for x in 0..target.w as usize {
            local.set(x, y, engine_render::dots::Dot::Lit(UNDERLINE_COLOR));
        }
    }
    crate::scenes::post_battle::columns::blit_dots(buf, target, &local);
}

/// Calls [`wrap`] and paints into `area` (a cell rect): text row `r` lands
/// on glyph cell-row `area.y + r*2`, with any blank's underline in the
/// cell-row directly beneath (`area.y + r*2 + 1`), painted through the
/// braille dot pipeline. The active caret cell (per `caret`) is painted
/// `Modifier::REVERSED` when `caret.visible`. No-ops on a degenerate `area`.
pub(crate) fn render(buf: &mut Buffer, area: Rect, runs: &[ParaRun], caret: Option<Caret>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let pieces = wrap(runs, area.width, caret.as_ref());
    let text_style = Style::default().fg(Color::Rgb(TEXT_COLOR.r, TEXT_COLOR.g, TEXT_COLOR.b));

    for piece in &pieces {
        let glyph_row = area.y.saturating_add(piece.row.saturating_mul(2));
        if glyph_row >= area.bottom() {
            continue;
        }
        let col = area.x.saturating_add(piece.col);

        if !piece.text.is_empty() {
            buf.set_stringn(col, glyph_row, &piece.text, piece.cells as usize, text_style);
        }

        if piece.underline && piece.cells > 0 {
            let underline_row = glyph_row.saturating_add(1);
            if underline_row < area.bottom() {
                draw_underline(buf, col, underline_row, piece.cells);
            }
        }
    }

    if let Some(caret) = caret {
        if caret.visible {
            if let Some((col, row)) = caret_cell(&pieces, area, &caret) {
                if let Some(cell) = buf.cell_mut((col, row)) {
                    cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
                }
            }
        }
    }
}

/// The cell-row height (`2 * row_count`) [`wrap`] would occupy at `width`,
/// so callers can size a body region before painting; `0` for empty `runs`.
pub(crate) fn measure_height(runs: &[ParaRun], width: u16) -> u16 {
    match wrap(runs, width, None).iter().map(|p| p.row).max() {
        Some(max_row) => (max_row + 1) * 2,
        None => 0,
    }
}
