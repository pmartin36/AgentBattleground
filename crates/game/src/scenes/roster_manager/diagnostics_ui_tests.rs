//! Tests for `diagnostics_ui`: the `[!!]` warning badge, the warning card's
//! content (pluralized count header + one wrapped entry per diagnostic), and
//! the byte-span -> editor `(line, col)` mapping behind `DiagnosticDecorations`.
//!
//! `byte_to_pos` is private, so it is exercised only through `span_to_line_col`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::diagnostics_ui::{
    badge_rect, count_header, decorations_for, draw_badge, layout_warning_card,
    render_warning_card, span_to_line_col, BADGE_TEXT, WARNING_COLOR,
};
use super::tooltip::{ANCHOR_GAP_CELLS, TOOLTIP_WIDTH_CELLS};
use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::scenes::test_util::{has_non_space, rect_text, region_cells, sample_fg};
use engine_render::{DotRect, Sizing, TextEditor, TextEditorConfig};

fn one_diagnostic() -> Vec<Diagnostic> {
    vec![Diagnostic::new(DiagnosticKind::UnknownTarget { token: "foo".to_string() }, None)]
}

fn five_diagnostics() -> Vec<Diagnostic> {
    (0..5)
        .map(|i| {
            Diagnostic::new(
                DiagnosticKind::UnknownTarget { token: format!("t{i}") },
                None,
            )
        })
        .collect()
}

#[test]
fn badge_text_is_four_ascii_cells() {
    assert!(BADGE_TEXT.is_ascii(), "BADGE_TEXT must be ASCII, was {:?}", BADGE_TEXT);
    assert_eq!(BADGE_TEXT.chars().count(), 4, "BADGE_TEXT must be exactly 4 cells wide");
}

#[test]
fn badge_renders_text_in_warning_color() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
    let area = Rect::new(0, 0, 20, 3);
    let rect = draw_badge(&mut buf, area, &one_diagnostic()).expect("should draw with >= 1 diagnostic");

    assert!(rect_text(&buf, rect).contains(BADGE_TEXT), "expected {:?} in rect, got {:?}", BADGE_TEXT, rect_text(&buf, rect));
    let expected_color = Color::Rgb(WARNING_COLOR.r, WARNING_COLOR.g, WARNING_COLOR.b);
    assert_eq!(sample_fg(&buf, rect), Some(expected_color));
}

#[test]
fn no_diagnostics_writes_no_badge_cell() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
    let area = Rect::new(0, 0, 20, 3);
    let result = draw_badge(&mut buf, area, &[]);

    assert_eq!(result, None, "must return None when there are no diagnostics");
    assert!(!has_non_space(&buf, area), "no cell in area should be written when there are no diagnostics");
}

#[test]
fn returned_rect_is_exactly_the_drawn_cells() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 5));
    let area = Rect::new(2, 2, 10, 1);
    let rect = draw_badge(&mut buf, area, &one_diagnostic()).expect("should draw");

    // Every cell inside the returned rect is non-space.
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            assert_ne!(
                buf.cell((x, y)).unwrap().symbol(),
                " ",
                "cell ({x},{y}) inside returned rect must be non-space"
            );
        }
    }

    // Cells immediately left/right/above/below the returned rect, but still
    // inside `area`, are blank.
    let neighbors: Vec<(u16, u16)> = [
        rect.left().checked_sub(1).map(|x| (x, rect.top())),
        Some((rect.right(), rect.top())),
        rect.top().checked_sub(1).map(|y| (rect.left(), y)),
        Some((rect.left(), rect.bottom())),
    ]
    .into_iter()
    .flatten()
    .filter(|&(x, y)| x >= area.left() && x < area.right() && y >= area.top() && y < area.bottom())
    .collect();
    for (x, y) in neighbors {
        assert_eq!(
            buf.cell((x, y)).unwrap().symbol(),
            " ",
            "cell ({x},{y}) just outside the returned rect (but inside area) must be blank"
        );
    }
}

#[test]
fn badge_is_identical_regardless_of_diagnostic_count() {
    let area = Rect::new(0, 0, 20, 3);

    let mut buf_one = Buffer::empty(area);
    let rect_one = draw_badge(&mut buf_one, area, &one_diagnostic()).expect("should draw");

    let mut buf_five = Buffer::empty(area);
    let rect_five = draw_badge(&mut buf_five, area, &five_diagnostics()).expect("should draw");

    assert_eq!(rect_one, rect_five, "returned rect must not depend on diagnostic count");
    assert_eq!(
        region_cells(&buf_one, area),
        region_cells(&buf_five, area),
        "rendered cells must be identical regardless of diagnostic count"
    );
}

#[test]
fn degenerate_area_returns_none() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 20));

    assert_eq!(badge_rect(Rect::new(0, 0, 0, 5)), None, "zero-width area");
    assert_eq!(badge_rect(Rect::new(0, 0, 5, 0)), None, "zero-height area");

    assert_eq!(
        draw_badge(&mut buf, Rect::new(0, 0, 0, 5), &one_diagnostic()),
        None,
        "zero-width area must not draw or panic"
    );
    assert_eq!(
        draw_badge(&mut buf, Rect::new(0, 0, 5, 0), &one_diagnostic()),
        None,
        "zero-height area must not draw or panic"
    );
    // Off-buffer area: fully outside the 20x20 buffer.
    assert_eq!(
        draw_badge(&mut buf, Rect::new(100, 100, 5, 1), &one_diagnostic()),
        None,
        "fully off-buffer area must not draw or panic"
    );
}

// ---------------------------------------------------------------------
// b3-t3: warning card content (count header + one entry per diagnostic).
// ---------------------------------------------------------------------

/// Anchor far from every screen edge, matching `shell.rs`'s `far_anchor()`
/// / `tooltip/mod.rs`'s `hovered_cell()` — the near-edge clamp is b3-t1's
/// contract and must not be re-tested here.
fn far_anchor() -> DotRect {
    DotRect { x: 200, y: 120, w: 20, h: 16 }
}

/// A buffer large enough to hold the anchored card for `far_anchor()`,
/// matching `tooltip/mod.rs`'s `tooltip_test_buf()` shape.
fn card_test_buf() -> Buffer {
    Buffer::empty(Rect::new(0, 0, 110, 32))
}

fn diagnostic(kind: DiagnosticKind) -> Diagnostic {
    Diagnostic::new(kind, None)
}

fn diagnostics_with_distinct_tokens(n: usize) -> Vec<Diagnostic> {
    (0..n)
        .map(|i| diagnostic(DiagnosticKind::CreatureNotOnRoster { token: format!("Token{i}") }))
        .collect()
}

/// `rect_text` joins rows with no separator, and the card's `Occlude` fill
/// is U+2800 (braille blank), NOT " " — so a raw `contains(&msg)` is false
/// for any message that wraps. Normalize a ROW rect (never the card rect:
/// the frame's border glyphs sit between rows) before asserting.
fn row_text(buf: &Buffer, rect: Rect) -> String {
    rect_text(buf, rect).replace('\u{2800}', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn count_header_pluralizes_at_the_boundary() {
    assert_eq!(count_header(1), "1 issue");
    assert_ne!(count_header(1), "1 issues", "must not pluralize the singular");
    assert_eq!(count_header(2), "2 issues");
}

#[test]
fn warning_card_renders_pluralized_count_header() {
    let one = diagnostics_with_distinct_tokens(1);
    let mut buf = card_test_buf();
    render_warning_card(&mut buf, far_anchor(), &one);
    let layout = layout_warning_card(far_anchor(), &one);
    assert_eq!(row_text(&buf, layout.rows[0].to_cell_rect()), "1 issue");

    let two = diagnostics_with_distinct_tokens(2);
    let mut buf = card_test_buf();
    render_warning_card(&mut buf, far_anchor(), &two);
    let layout = layout_warning_card(far_anchor(), &two);
    assert_eq!(row_text(&buf, layout.rows[0].to_cell_rect()), "2 issues");
}

#[test]
fn warning_card_has_one_row_per_diagnostic_plus_header() {
    for n in [1, 2, 5] {
        let diags = diagnostics_with_distinct_tokens(n);
        let layout = layout_warning_card(far_anchor(), &diags);
        assert_eq!(layout.rows.len(), n + 1, "n={n}: rows must be n diagnostics + 1 header");
    }
}

#[test]
fn warning_card_renders_every_diagnostic_message_in_its_own_row() {
    let diags = diagnostics_with_distinct_tokens(3);
    let mut buf = card_test_buf();
    render_warning_card(&mut buf, far_anchor(), &diags);
    let layout = layout_warning_card(far_anchor(), &diags);

    for (i, diag) in diags.iter().enumerate() {
        let text = row_text(&buf, layout.rows[i + 1].to_cell_rect());
        assert!(
            text.contains(&diag.message),
            "row {i} must contain its own full message {:?}; got {text:?}",
            diag.message
        );
        for j in 0..diags.len() {
            if i == j {
                continue;
            }
            assert!(
                !text.contains(&format!("Token{j}")),
                "row {i} must not contain another diagnostic's token (Token{j})"
            );
        }
    }
}

#[test]
fn every_diagnostic_kind_message_renders_without_truncation() {
    let kinds = [
        DiagnosticKind::PromptTooLong { words: 1004, limit: 500 },
        DiagnosticKind::UnknownTarget { token: "foo".to_string() },
        DiagnosticKind::BadSelectorForTarget { token: "most-hp".to_string() },
        DiagnosticKind::NotAnAbility { token: "Frost_Bite".to_string() },
        DiagnosticKind::CreatureNotOnRoster { token: "Volt_Scorpion".to_string() },
        DiagnosticKind::CreatureNotFielded { token: "Verdant_Treant".to_string() },
    ];

    for kind in kinds {
        let diags = vec![diagnostic(kind)];
        let mut buf = card_test_buf();
        render_warning_card(&mut buf, far_anchor(), &diags);
        let layout = layout_warning_card(far_anchor(), &diags);

        let text = row_text(&buf, layout.rows[1].to_cell_rect());
        assert!(
            text.contains(&diags[0].message),
            "entry must contain the full message {:?}; got {text:?}",
            diags[0].message
        );
        assert!(!text.contains('…'), "message must not be truncated; got {text:?}");
    }
}

#[test]
fn warning_card_occludes_content_beneath_it() {
    let diags = diagnostics_with_distinct_tokens(1);
    let layout = layout_warning_card(far_anchor(), &diags);
    let card_rect = layout.card.to_cell_rect();
    let probe = (card_rect.x + card_rect.width / 2, card_rect.y + card_rect.height / 2);

    let mut buf = card_test_buf();
    if let Some(cell) = buf.cell_mut(probe) {
        cell.set_char('Q');
    }

    render_warning_card(&mut buf, far_anchor(), &diags);

    assert_ne!(
        buf.cell(probe).unwrap().symbol(),
        "Q",
        "cell under the card must no longer show pre-existing content"
    );
}

#[test]
fn warning_card_is_tooltip_width_and_anchored_up_left() {
    let diags = diagnostics_with_distinct_tokens(1);
    let layout = layout_warning_card(far_anchor(), &diags);
    let anchor = far_anchor();

    assert_eq!(layout.card.w, TOOLTIP_WIDTH_CELLS as i32 * 2);
    assert_eq!(
        layout.card.x + layout.card.w,
        anchor.x + anchor.w / 2 - ANCHOR_GAP_CELLS as i32 * 2,
        "card must anchor up-left of the anchor's horizontal center"
    );
}

#[test]
fn no_diagnostics_renders_no_card() {
    let one = diagnostics_with_distinct_tokens(1);
    let region = layout_warning_card(far_anchor(), &one).card.to_cell_rect();

    let mut buf = card_test_buf();
    render_warning_card(&mut buf, far_anchor(), &[]);

    assert!(
        !has_non_space(&buf, region),
        "no cell in the card's footprint should be written when there are no diagnostics"
    );
}

// ---------------------------------------------------------------------
// b3-t4: byte-span -> editor (line, col) mapping.
// ---------------------------------------------------------------------

fn spanned(kind: DiagnosticKind, span: std::ops::Range<usize>) -> Diagnostic {
    Diagnostic::new(kind, Some(span))
}

fn plain_editor() -> TextEditor {
    TextEditor::new(TextEditorConfig {
        sizing: Sizing::Fixed,
        submit_on_enter: false,
        placeholder: String::new(),
    })
}

const BG: Color = Color::Rgb(10, 20, 30);

#[test]
fn multibyte_before_span_does_not_shift_it() {
    let text = "café — @Volt_Scorpion";
    let start = text.find('@').expect("test text must contain @");
    let end = start + "@Volt_Scorpion".len();

    let span = span_to_line_col(text, &(start..end)).expect("span must be on `text`");

    assert_eq!(span.0, (0, 7), "byte col must not be used as the char col");
}

#[test]
fn span_on_later_line_maps_to_that_line() {
    let text = "line0\nline1 @Target\nline2";
    let start = text.find('@').expect("test text must contain @");
    let end = start + "@Target".len();

    let span = span_to_line_col(text, &(start..end)).expect("span must be on `text`");

    assert_eq!(span, ((1, 6), (1, 13)));
}

#[test]
fn prompt_too_long_span_none_yields_no_decoration() {
    let text = "anything";
    let only_unspanned = vec![diagnostic(DiagnosticKind::PromptTooLong { words: 1004, limit: 500 })];
    assert_eq!(decorations_for(text, &only_unspanned, BG).decorations(), vec![]);

    let mixed = vec![
        diagnostic(DiagnosticKind::PromptTooLong { words: 1004, limit: 500 }),
        spanned(DiagnosticKind::UnknownTarget { token: "x".to_string() }, 0..4),
    ];
    let decorations = decorations_for(text, &mixed, BG).decorations();
    assert_eq!(decorations.len(), 1, "PromptTooLong must not contribute a decoration");
    assert_eq!(
        decorations[0].span,
        span_to_line_col(text, &(0..4)).unwrap(),
        "the remaining decoration must be the spanned diagnostic's own span"
    );
}

/// The routed defect (`scope/bucket-b3.md`, SEVERITY med): a decoration
/// index is NOT the same as its diagnostic's index once a span-less
/// diagnostic has been filtered out. Reuses the exact desync shape from
/// `prompt_too_long_span_none_yields_no_decoration` above.
#[test]
fn decoration_index_maps_back_to_its_diagnostic() {
    let text = "anything";
    let diags = vec![
        diagnostic(DiagnosticKind::PromptTooLong { words: 1004, limit: 500 }),
        spanned(DiagnosticKind::UnknownTarget { token: "x".to_string() }, 0..4),
    ];

    let decos = decorations_for(text, &diags, BG);

    assert_eq!(decos.decorations().len(), 1);
    assert_eq!(
        decos.diagnostic_index(0),
        Some(1),
        "the sole decoration came from diags[1] (PromptTooLong at index 0 was filtered out)"
    );
    assert_eq!(decos.diagnostic_index(1), None, "out-of-range decoration index must be None");
}

/// Oracle test against the actual consumer: `agrees_with_a_real_text_editor`
/// cannot catch a renumbering bug because it uses a single diagnostic, where
/// index 0 aligns by coincidence. This uses a span-less diagnostic FIRST,
/// then two spanned diagnostics on different lines, so a naive
/// `decoration_at(pos) -> diagnostics[idx]` reading would resolve to the
/// wrong message.
#[test]
fn hover_index_from_a_real_editor_selects_the_right_diagnostic() {
    let text = "line0 @First\nline1 @Second";
    let first_start = text.find("@First").expect("test text must contain @First");
    let first_end = first_start + "@First".len();
    let second_start = text.find("@Second").expect("test text must contain @Second");
    let second_end = second_start + "@Second".len();

    let diags = vec![
        diagnostic(DiagnosticKind::PromptTooLong { words: 1004, limit: 500 }),
        spanned(DiagnosticKind::UnknownTarget { token: "First".to_string() }, first_start..first_end),
        spanned(
            DiagnosticKind::CreatureNotOnRoster { token: "Second".to_string() },
            second_start..second_end,
        ),
    ];

    let decos = decorations_for(text, &diags, BG);
    let mut editor = plain_editor();
    editor.set_text(text);
    editor.set_decorations(decos.decorations());

    let (second_line_col_start, _) = span_to_line_col(text, &(second_start..second_end)).unwrap();
    let deco_idx = editor
        .decoration_at(second_line_col_start)
        .expect("the second span must be decorated");
    let diag_idx = decos
        .diagnostic_index(deco_idx)
        .expect("decoration index must map back to a diagnostic index");

    assert_eq!(diag_idx, 2, "hovering the second span must resolve to diags[2], not diags[1]");
    assert!(
        diags[diag_idx].message.contains("Second"),
        "resolved message must be the second token's, got {:?}",
        diags[diag_idx].message
    );
    assert!(
        !diags[diag_idx].message.contains("First"),
        "resolved message must NOT be the first token's, got {:?}",
        diags[diag_idx].message
    );
}

#[test]
fn mapping_matches_set_text_line_split_on_crlf() {
    let text = "hdr\r\n@Volt_Scorpion x";
    let start = text.find('@').expect("test text must contain @");
    let end = start + "@Volt_Scorpion".len();

    let span = span_to_line_col(text, &(start..end)).expect("span must be on `text`");

    assert_eq!(span.0, (1, 0), "must follow split('\\n'), not str::lines()");
}

#[test]
fn trailing_newline_final_empty_line_is_addressable() {
    let text = "alpha\n";
    let end = text.len();

    let span = span_to_line_col(text, &(end..end)).expect("end-of-text position must be addressable");

    assert_eq!(span, ((1, 0), (1, 0)), "the final empty line after a trailing newline must exist");
}

#[test]
fn agrees_with_a_real_text_editor() {
    let text = "café — @Volt_Scorpion";
    let start = text.find('@').expect("test text must contain @");
    let end = start + "@Volt_Scorpion".len();
    let diags = vec![spanned(DiagnosticKind::UnknownTarget { token: "Volt_Scorpion".to_string() }, start..end)];

    let decorations = decorations_for(text, &diags, BG).decorations();
    let mut editor = plain_editor();
    editor.set_text(text);
    editor.set_decorations(decorations);

    let (line_col_start, line_col_end) = span_to_line_col(text, &(start..end)).unwrap();
    assert_eq!(editor.decoration_at(line_col_start), Some(0), "start of the span must be decorated");
    assert_eq!(editor.decoration_at(line_col_end), None, "end must be exclusive, per the editor's own convention");
}

#[test]
fn span_end_is_exclusive() {
    let text = "abc @Foo def";
    let start = text.find('@').expect("test text must contain @");
    let end = start + "@Foo".len();

    let span = span_to_line_col(text, &(start..end)).expect("span must be on `text`");

    assert_eq!(span.1, (0, end), "end col must be one past the token's last char");
    assert_eq!(text.chars().nth(end - 1), Some('o'), "sanity: last char of the token is 'o'");
    assert_eq!(text.chars().nth(end), Some(' '), "sanity: the char AT end col is past the token");
}

#[test]
fn out_of_range_or_non_boundary_span_is_none_not_panic() {
    assert_eq!(span_to_line_col("abc", &(0..99)), None, "out-of-range end must be None, not panic");

    let text = "café";
    let e_byte = text.find('é').unwrap();
    assert_eq!(
        span_to_line_col(text, &(e_byte..e_byte + 1)),
        None,
        "a non-char-boundary offset inside a multi-byte char must be None, not panic"
    );
}
