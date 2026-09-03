//! Inline edit-mode input routing: typed keys land in the active blank,
//! Tab/Shift-Tab cycle which blank is active without ever switching the
//! selected egg, Esc returns to browsing, and the gated submit composes the
//! blanks into the completed sentence and drives the Done pipeline only
//! once every blank holds non-empty text.

use super::*;
use crate::text_gen::backend::TextBackend;
use crate::text_gen::job::CancelFlag as TextCancelFlag;
use crate::text_gen::operation::TextGen;
use crate::text_gen::{Provider, ResolvedModelConfig, TextError, TextRequest};
use crossterm::event::KeyCode;

/// A `TextBackend` fixture that always returns a fixed parts completion, so
/// a submitted parts-text job never touches a real backend.
struct FixedBackend(String);
impl TextBackend for FixedBackend {
    fn generate(&self, _request: &TextRequest, _cancel: &TextCancelFlag) -> Result<String, TextError> {
        Ok(self.0.clone())
    }
}

fn fixed_text_gen_factory() -> super::super::TextGenFactory {
    Box::new(|cfg: &ResolvedModelConfig| {
        TextGen::with_backend_factory(
            cfg.clone(),
            Box::new(|_cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                Box::new(FixedBackend(
                    "NAME: Ember\nDESCRIPTION: A tiny beast.\nARCHETYPE: Melee\n".to_string(),
                ))
            }),
            std::time::Duration::from_secs(2),
        )
    })
}

fn present_model_config() -> ResolvedModelConfig {
    ResolvedModelConfig::new(Provider::Local, "m", None, Some("bin".to_string()), None)
}

/// A hermetic `Hatchery` with a single `Undefined` egg (template 0, whose
/// three blanks are "size", "temperament", "signature move"), a resolved
/// model config, and a fixed-backend text-gen factory, already in edit mode
/// for that egg.
fn scene_editing_undefined_egg(tag: &str) -> Hatchery {
    let dir = temp_store_dir(tag);
    let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_with_gen(
        PlayerStore::with_dir(&dir),
        SystemTime::now(),
        Hatchery::production_asset_gen(),
        Ok(present_model_config()),
        fixed_text_gen_factory(),
    );
    scene.enter_edit(0);
    scene
}

/// Sets blank `i`'s text directly (bypassing input routing) so submit-gate
/// tests can seed blank state without depending on the typing path.
fn set_blank_text(scene: &Hatchery, i: usize, text: &str) {
    scene.blank_editors[i].borrow_mut().set_text(text);
}

/// Entering edit mode on an undefined egg paints its mad-lib into the
/// reserved detail body region: even with every blank empty, the first
/// blank's minimum-floor underline is a lit braille dot somewhere in that
/// region, painted through the dot pipeline.
#[test]
fn entering_edit_renders_underline_dots_in_detail_body() {
    let scene = scene_editing_undefined_egg("render-body");
    let (w, h) = (60u16, 24u16);
    let buf = render_to_buffer(&scene, w, h);
    let area = Rect::new(0, 0, w, h);
    let (_egg, body, _tray) = detail_layout::detail_layout(area);

    // Match the underline's own exact gray, not just any lit dot, so a
    // tray/egg-sprite fringe bleeding one row into `body` cannot pass this
    // test in place of the mad-lib actually rendering.
    let underline = mad_lib_paragraph::UNDERLINE_COLOR;
    let mut found = false;
    'scan: for y in body.top()..body.bottom() {
        for x in body.left()..body.right() {
            if let Some((mask, color)) = engine_render::decode_braille_cell(&buf, x, y) {
                if mask != 0 && color.r == underline.r && color.g == underline.g && color.b == underline.b {
                    found = true;
                    break 'scan;
                }
            }
        }
    }
    assert!(
        found,
        "editing an undefined egg must paint its mad-lib's blank underline (exact color {underline:?}) \
         into the detail body region"
    );
}

/// Typing while editing routes each character to the active (first) blank's
/// `TextEditor`, leaving the other blanks untouched.
#[test]
fn typing_routes_to_the_active_blank() {
    let mut scene = scene_editing_undefined_egg("typing-routes");

    for ch in "big".chars() {
        scene.handle_input(key_event(KeyCode::Char(ch)));
    }

    assert_eq!(
        scene.blank_editors[0].borrow().text(),
        "big",
        "typed characters must land in the active (first) blank"
    );
    assert_eq!(
        scene.blank_editors[1].borrow().text(),
        "",
        "an inactive blank must receive no typed input"
    );
}

/// Tab while editing moves to the next blank without ever changing the
/// selected egg.
#[test]
fn tab_cycles_the_active_blank_forward_without_changing_selection() {
    let mut scene = scene_editing_undefined_egg("tab-forward");

    scene.handle_input(key_event(KeyCode::Tab));

    assert!(
        matches!(scene.mode, HatcheryMode::Editing { active_blank: 1 }),
        "Tab while editing must move to the next blank, got {:?}",
        scene.mode
    );
    assert_eq!(scene.selected, Some(0), "Tab while editing must never change the selected egg");
}

/// Shift-Tab (BackTab) from the first blank wraps the active blank to the
/// last one.
#[test]
fn shift_tab_wraps_the_active_blank_to_the_last() {
    let mut scene = scene_editing_undefined_egg("backtab-wrap");

    scene.handle_input(key_event(KeyCode::BackTab));

    assert!(
        matches!(scene.mode, HatcheryMode::Editing { active_blank: 2 }),
        "Shift-Tab from the first blank must wrap to the last blank, got {:?}",
        scene.mode
    );
}

/// Esc while editing returns to Browsing and leaves the egg undefined.
#[test]
fn esc_leaves_edit_mode_and_keeps_the_egg_undefined() {
    let mut scene = scene_editing_undefined_egg("esc-leaves-edit");

    scene.handle_input(key_event(KeyCode::Esc));

    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { .. }),
        "Esc while editing must return to Browsing, got {:?}",
        scene.mode
    );
    assert_eq!(scene.eggs[0].state, EggState::Undefined, "Esc must leave the egg undefined");
}

/// `try_submit_edit` is inert while any blank is still empty.
#[test]
fn try_submit_edit_is_inert_while_any_blank_is_empty() {
    let mut scene = scene_editing_undefined_egg("submit-inert");
    set_blank_text(&scene, 0, "big");
    set_blank_text(&scene, 1, "calm");
    // blank 2 stays empty

    let submitted = scene.try_submit_edit();

    assert!(!submitted, "try_submit_edit must return false while any blank is empty");
    assert!(scene.definition.is_none(), "an incomplete submit must not start the Done pipeline");
    assert_eq!(scene.eggs[0].state, EggState::Undefined, "an incomplete submit must leave the egg undefined");
}

/// A whitespace-only blank counts as empty for the submit gate, exactly
/// like a blank with no text at all.
#[test]
fn try_submit_edit_treats_whitespace_only_blank_as_empty() {
    let mut scene = scene_editing_undefined_egg("submit-whitespace");
    set_blank_text(&scene, 0, "big");
    set_blank_text(&scene, 1, "   ");
    set_blank_text(&scene, 2, "tail whip");

    let submitted = scene.try_submit_edit();

    assert!(!submitted, "a whitespace-only blank must count as empty, not filled");
    assert!(scene.definition.is_none(), "a whitespace-gated submit must not start the Done pipeline");
}

/// Once every blank is filled, `try_submit_edit` composes the sentence via
/// `mad_lib::completed_sentence` and drives the Done pipeline.
#[test]
fn try_submit_edit_composes_and_submits_once_every_blank_is_filled() {
    let mut scene = scene_editing_undefined_egg("submit-complete");
    set_blank_text(&scene, 0, "big");
    set_blank_text(&scene, 1, "calm");
    set_blank_text(&scene, 2, "tail whip");
    let expected = mad_lib::completed_sentence(mad_lib::select_template(0), &["big", "calm", "tail whip"]);

    let submitted = scene.try_submit_edit();

    assert!(submitted, "try_submit_edit must return true once every blank is filled");
    assert_eq!(
        scene.eggs[0].mad_lib.as_deref(),
        Some(expected.as_str()),
        "the submitted sentence must be composed from the blank values via mad_lib::completed_sentence"
    );
    assert!(scene.definition.is_some(), "a complete submit must start the Done pipeline");
}
