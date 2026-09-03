//! Decode-verified tests for the mad-lib paragraph model: `wrap`'s pure
//! word-boundary layout (literal and blank runs alike), the braille-dot
//! underline (floor, growth, per-row alignment under a wrapped blank's own
//! words), the read-only all-literal render, and the `Modifier::REVERSED`
//! caret reusing a `TextEditor`'s own blink phase.

use std::collections::HashSet;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use engine_render::{Sizing, TextEditor, TextEditorConfig};

use super::mad_lib_paragraph::{render, wrap, Caret, ParaRun, Piece, MIN_UNDERLINE_CELLS};

fn new_editor() -> TextEditor {
    TextEditor::new(TextEditorConfig {
        sizing: Sizing::Fixed,
        submit_on_enter: false,
        placeholder: String::new(),
    })
}

/// Concatenates every non-empty piece's text, in the order `wrap` returned
/// them, joined by a single space — a grouping-agnostic reconstruction that
/// lets word-boundary assertions hold regardless of how many words a piece
/// batches together.
fn reconstruct_words(pieces: &[Piece]) -> Vec<String> {
    pieces
        .iter()
        .filter(|p| !p.text.is_empty())
        .map(|p| p.text.clone())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// A narrow paragraph of literal words wraps across multiple rows, breaking
/// only at whitespace: every piece stays within `width` and the words
/// reconstruct in original order with none split mid-word.
#[test]
fn literal_paragraph_wraps_at_word_boundaries_across_rows() {
    let runs = [ParaRun::Literal("alpha beta gamma delta epsilon")];
    let width = 12;

    let pieces = wrap(&runs, width, None);

    assert!(!pieces.is_empty(), "a non-empty paragraph must produce at least one piece");
    let max_row = pieces.iter().map(|p| p.row).max().unwrap();
    assert!(max_row >= 1, "a paragraph this long at width {width} must wrap across >=2 rows");
    for p in &pieces {
        assert!(
            p.col + p.cells <= width,
            "piece {p:?} exceeds width {width}"
        );
    }
    assert_eq!(
        reconstruct_words(&pieces),
        vec!["alpha", "beta", "gamma", "delta", "epsilon"],
        "wrapping must never split a word or reorder words"
    );
}

/// A multi-word blank value wraps across a row break exactly like literal
/// text: its words land on >= 2 rows, at a word boundary, each carrying the
/// underline flag.
#[test]
fn blank_words_wrap_like_literal_words_at_a_word_boundary() {
    let runs =
        [ParaRun::Literal("A"), ParaRun::Blank("burst of radiant light"), ParaRun::Literal("today.")];
    let width = 10;

    let pieces = wrap(&runs, width, None);

    let blank_pieces: Vec<_> =
        pieces.iter().filter(|p| matches!(p.blank, Some((0, _))) && !p.text.is_empty()).collect();
    assert!(!blank_pieces.is_empty(), "the blank's words must appear in the wrapped pieces");
    let rows: HashSet<u16> = blank_pieces.iter().map(|p| p.row).collect();
    assert!(
        rows.len() >= 2,
        "a blank this long at width {width} must wrap across >=2 rows, got rows {rows:?}"
    );
    assert!(
        blank_pieces.iter().all(|p| p.underline),
        "every glyph piece belonging to a blank must carry the underline flag"
    );
    let words: Vec<String> = blank_pieces
        .iter()
        .map(|p| p.text.clone())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .map(str::to_string)
        .collect();
    assert_eq!(
        words,
        vec!["burst", "of", "radiant", "light"],
        "the blank's own words must wrap intact and in order"
    );
}

/// Rendering that same wrapped, multi-row blank must paint a lit underline
/// dot in the cell-row directly beneath EVERY cell of the blank's glyphs on
/// EACH row it occupies — alignment verified by decoding the dots, not by
/// comparing rect coordinates.
#[test]
fn wrapped_blank_has_underline_dots_under_its_words_on_each_row() {
    let runs =
        [ParaRun::Literal("A"), ParaRun::Blank("burst of radiant light"), ParaRun::Literal("today.")];
    let width = 10;
    let area = Rect::new(0, 0, width, 40);

    let pieces = wrap(&runs, width, None);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &runs, None);

    let blank_pieces: Vec<_> =
        pieces.iter().filter(|p| matches!(p.blank, Some((0, _))) && !p.text.is_empty()).collect();
    let rows: HashSet<u16> = blank_pieces.iter().map(|p| p.row).collect();
    assert!(rows.len() >= 2, "setup must exercise a blank spanning >=2 rows");

    for p in &blank_pieces {
        let underline_row = area.y + p.row * 2 + 1;
        for dx in 0..p.cells {
            let col = p.col + dx;
            assert!(
                crate::scenes::test_util::braille_mask(&buf, col, underline_row).is_some(),
                "row {}: expected a lit underline dot beneath blank glyph col {col}",
                p.row
            );
        }
    }
}

/// An empty blank still shows an underline at least `MIN_UNDERLINE_CELLS`
/// wide (the floor), decoded from the rendered dots.
#[test]
fn empty_blank_shows_at_least_min_floor_underline_width() {
    let runs = [ParaRun::Blank("")];
    let width = 20;
    let area = Rect::new(0, 0, width, 4);

    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &runs, None);

    let count = (0..width)
        .filter(|&col| crate::scenes::test_util::braille_mask(&buf, col, area.y + 1).is_some())
        .count();
    assert!(
        count as u16 >= MIN_UNDERLINE_CELLS,
        "empty blank underline was {count} cells, expected >= {MIN_UNDERLINE_CELLS}"
    );
}

/// A blank value longer than the floor renders a wider underline than the
/// empty case — the underline grows with the text instead of staying
/// pinned to the floor.
#[test]
fn longer_blank_value_produces_wider_underline_than_empty() {
    let width = 40;
    let area = Rect::new(0, 0, width, 4);

    let empty_runs = [ParaRun::Blank("")];
    let mut empty_buf = Buffer::empty(area);
    render(&mut empty_buf, area, &empty_runs, None);
    let empty_count = (0..width)
        .filter(|&col| crate::scenes::test_util::braille_mask(&empty_buf, col, area.y + 1).is_some())
        .count();

    let value = "gigantically-oversized";
    let long_runs = [ParaRun::Blank(value)];
    let mut long_buf = Buffer::empty(area);
    render(&mut long_buf, area, &long_runs, None);
    let long_count = (0..width)
        .filter(|&col| crate::scenes::test_util::braille_mask(&long_buf, col, area.y + 1).is_some())
        .count();

    assert!(
        long_count > empty_count,
        "a longer blank value ({long_count} cells) must render a wider underline than empty ({empty_count} cells)"
    );
    assert_eq!(
        long_count,
        value.chars().count(),
        "underline width past the floor must track the value's own width"
    );
}

/// A single-word blank's underline sits directly beneath its own glyph
/// cells: for every column the blank's glyphs occupy, the cell-row directly
/// beneath carries a lit dot at that SAME column — checked by decoding the
/// dots, not by comparing `Rect`/`DotRect` fields.
#[test]
fn underline_sits_directly_beneath_its_glyph_cells() {
    let runs = [ParaRun::Literal("Its"), ParaRun::Blank("razor"), ParaRun::Literal("claws.")];
    let width = 30;
    let area = Rect::new(0, 0, width, 4);

    let pieces = wrap(&runs, width, None);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &runs, None);

    let blank_piece = pieces
        .iter()
        .find(|p| matches!(p.blank, Some((0, _))) && !p.text.is_empty())
        .expect("the blank's glyph piece must be present");
    let glyph_row = area.y + blank_piece.row * 2;
    let underline_row = glyph_row + 1;
    for dx in 0..blank_piece.cells {
        let col = blank_piece.col + dx;
        assert!(
            crate::scenes::test_util::braille_mask(&buf, col, underline_row).is_some(),
            "column {col}: underline dot must sit directly beneath the glyph at row {glyph_row}"
        );
    }
}

/// An all-literal (read-only) paragraph — a defined egg's completed
/// sentence — renders its words with no underline dots anywhere and no
/// `Modifier::REVERSED` cell.
#[test]
fn read_only_all_literal_paragraph_has_no_underline_dots_and_no_caret() {
    let runs = [ParaRun::Literal("A calm creature with kind eyes.")];
    let width = 20;
    let area = Rect::new(0, 0, width, 8);

    let mut buf = Buffer::empty(area);
    render(&mut buf, area, &runs, None);

    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            assert!(
                crate::scenes::test_util::braille_mask(&buf, x, y).is_none(),
                "read-only prose must carry no underline dots, found one at ({x}, {y})"
            );
            let cell = buf.cell((x, y)).unwrap();
            assert!(
                !cell.modifier.contains(Modifier::REVERSED),
                "read-only prose must carry no caret cell, found REVERSED at ({x}, {y})"
            );
        }
    }
}

/// The active blank's caret cell toggles `Modifier::REVERSED` between the
/// visible and hidden halves of a real `TextEditor`'s blink cycle — proving
/// the blink TIMING is reused, not reimplemented.
#[test]
fn active_blank_caret_toggles_reversed_across_blink_period() {
    let mut editor = new_editor();
    editor.set_text("hi");
    let cursor = editor.text().chars().count();
    let runs = [ParaRun::Blank("hi")];
    let width = 20;
    let area = Rect::new(0, 0, width, 4);

    let pieces = wrap(
        &runs,
        width,
        Some(&Caret { blank_ordinal: 0, cursor, visible: editor.caret_visible() }),
    );
    let caret_piece = pieces
        .iter()
        .find(|p| match p.blank {
            Some((0, start)) => cursor >= start && cursor <= start + p.cells as usize,
            _ => false,
        })
        .expect("a piece covering the end-of-text cursor must exist");
    let caret_col = caret_piece.col + (cursor - caret_piece.blank.unwrap().1) as u16;
    let glyph_row = area.y + caret_piece.row * 2;

    assert!(editor.caret_visible(), "a fresh editor starts in the visible blink phase");
    let mut buf_visible = Buffer::empty(area);
    render(
        &mut buf_visible,
        area,
        &runs,
        Some(Caret { blank_ordinal: 0, cursor, visible: editor.caret_visible() }),
    );
    assert!(
        buf_visible.cell((caret_col, glyph_row)).unwrap().modifier.contains(Modifier::REVERSED),
        "the caret cell must be REVERSED during the visible blink phase"
    );

    editor.tick(Duration::from_millis(650));
    assert!(!editor.caret_visible(), "650ms must cross into the hidden half of the 600ms blink period");
    let mut buf_hidden = Buffer::empty(area);
    render(
        &mut buf_hidden,
        area,
        &runs,
        Some(Caret { blank_ordinal: 0, cursor, visible: editor.caret_visible() }),
    );
    assert!(
        !buf_hidden.cell((caret_col, glyph_row)).unwrap().modifier.contains(Modifier::REVERSED),
        "the caret cell must NOT be REVERSED during the hidden blink phase"
    );
}

/// A caret targeting one blank must never mark a DIFFERENT blank's cells
/// REVERSED.
#[test]
fn inactive_blanks_show_no_caret() {
    let runs = [ParaRun::Blank("aa"), ParaRun::Literal("and"), ParaRun::Blank("bb")];
    let width = 30;
    let area = Rect::new(0, 0, width, 4);

    let pieces = wrap(&runs, width, None);
    let other_blank_pieces: Vec<_> =
        pieces.iter().filter(|p| matches!(p.blank, Some((1, _))) && !p.text.is_empty()).collect();
    assert!(!other_blank_pieces.is_empty(), "setup must place the second blank's glyphs somewhere");

    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &runs,
        Some(Caret { blank_ordinal: 0, cursor: 2, visible: true }),
    );

    for p in &other_blank_pieces {
        let glyph_row = area.y + p.row * 2;
        for dx in 0..p.cells {
            let col = p.col + dx;
            assert!(
                !buf.cell((col, glyph_row)).unwrap().modifier.contains(Modifier::REVERSED),
                "an inactive blank's cell ({col}, {glyph_row}) must never be REVERSED"
            );
        }
    }
}

/// `wrap`/`render` never panic on degenerate input: zero width, empty runs,
/// or a zero-area render target.
#[test]
fn wrap_and_render_do_not_panic_on_zero_width_or_empty_runs() {
    let runs = [ParaRun::Literal("hi")];

    assert!(wrap(&runs, 0, None).is_empty(), "zero width must yield no pieces");
    assert!(wrap(&[], 10, None).is_empty(), "empty runs must yield no pieces");

    let degenerate = Rect::new(0, 0, 0, 0);
    let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
    render(&mut buf, degenerate, &runs, None);
}
