//! Post-hatch Keep/Discard action: offers to place the freshly hatched
//! creature into the roster, or discard it, once the hatch sequence
//! completes. Keep places the hatchling either directly (an open slot) or
//! via a pick-a-creature-to-bump step when the roster is full; Discard
//! retires the egg without adding the hatchling. The pick step and the
//! bumped creature's disposal (`dispose_bumped`) stay two distinct steps so
//! the disposal can later be swapped for a move-to-Farm/Playpen action
//! without reworking the pick flow.

use std::cell::RefCell;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::Frame;

use engine_core::scene::{InputEvent, Transition};
use engine_render::{
    Align, Basis, Button, ButtonCore, ButtonState, Direction, FlexChild, FlexStyle, Justify, TextAlign,
};

use crate::player_data::PersistedCreature;
use crate::scenes::detail_panel;

use super::hatch_layout;

/// What the post-hatch UI shows, one state at a time.
pub(super) enum RosterAction {
    /// The hatch has completed; the Keep and Discard buttons are shown.
    Offer { keep: RefCell<Button>, discard: RefCell<Button> },
    /// The roster was full; the player is picking a creature to bump.
    Picking { roster: Vec<PersistedCreature>, buttons: RefCell<Vec<ButtonCore>> },
}

impl RosterAction {
    /// A fresh offer with both buttons idle.
    fn offer() -> Self {
        RosterAction::Offer {
            keep: RefCell::new(Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Keep")),
            discard: RefCell::new(Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Discard")),
        }
    }

    /// A fresh picker over `roster`, one hit-test core per candidate.
    fn picking(roster: Vec<PersistedCreature>) -> Self {
        let buttons = RefCell::new(roster.iter().map(|_| ButtonCore::new(Rect::default())).collect());
        RosterAction::Picking { roster, buttons }
    }
}

/// Discards a creature bumped out of a full roster. The single swap point
/// for a future move-to-Farm/Playpen action — the pick flow above never
/// changes when this body is replaced.
pub(super) fn dispose_bumped(_bumped: PersistedCreature) {}

/// Width, in cells, of each picker candidate row.
const BUTTON_W_CELLS: u16 = 18;
/// Gap, in cells, between the reveal's bottom edge and the picker panel
/// below it.
const GAP_CELLS: u16 = 1;
/// Height, in cells, of the picker's title row.
const PICKER_TITLE_H_CELLS: u16 = 1;
/// Height, in cells, of each picker candidate row.
const PICKER_ROW_H_CELLS: u16 = 2;
/// Gap, in dots, between the stacked Keep and Discard action rects. The
/// dock's reserved bottom slot is only a few dot-rows tall, so this stays at
/// 0 (the ability grid's own row gap) rather than a full cell — a wider gap
/// would floor one or both rows to a zero-height cell rect on `to_cell_rect`.
const DOCK_ACTION_GAP_DOTS: i32 = 0;

/// Rect for the full-roster pick panel, anchored below the reveal and
/// clamped to stay within `area`'s width.
pub(super) fn picker_panel_rect(area: Rect, focus_cell: Rect) -> Rect {
    let width = BUTTON_W_CELLS.min(area.width);
    let cx = focus_cell.x + focus_cell.width / 2;
    let x = cx.saturating_sub(width / 2).min(area.x + area.width.saturating_sub(width));
    let y = focus_cell.y + focus_cell.height + GAP_CELLS;
    let height = PICKER_TITLE_H_CELLS + PICKER_ROW_H_CELLS * crate::squad_role::ROSTER_SIZE as u16;
    Rect { x, y, width, height }
}

/// Rect for candidate button `i` within the pick panel.
pub(super) fn picker_button_rect(panel: Rect, i: usize) -> Rect {
    let y = panel.y + PICKER_TITLE_H_CELLS + PICKER_ROW_H_CELLS * i as u16;
    Rect { x: panel.x, y, width: panel.width, height: PICKER_ROW_H_CELLS }
}

/// What a completed click on the post-hatch UI resolved to, so the hit-test
/// (an `&self`-compatible borrow of `roster_action`) can finish and drop
/// before the resulting mutation runs.
enum PostHatchDecision {
    KeepClicked,
    DiscardClicked,
    Picked(usize),
}

impl super::Hatchery {
    /// Once the active hatch completes, offers the Keep/Discard action. A
    /// no-op once an action has already been set, or while no hatch is
    /// active/complete.
    pub(super) fn maybe_offer_dock_actions(&mut self) {
        let complete = self.hatch.as_ref().is_some_and(|h| h.seq.is_complete());
        if complete && self.roster_action.is_none() {
            self.roster_action = Some(RosterAction::offer());
        }
    }

    /// The settled dock's Keep/Discard action cell rects, in that order —
    /// the single authoritative source both the renderer (button
    /// `set_rect`) and hit-testing read, so the drawn rects and the tap
    /// targets can never drift apart. `None` without an active hatch and
    /// hatchling to place.
    pub(super) fn dock_action_rects(&self, area: Rect) -> Option<(Rect, Rect)> {
        let h = self.hatch.as_ref()?;
        let name = &self.eggs.get(h.egg)?.hatchling.as_ref()?.name;

        let (_focus_dr, strip) = super::focus::focus_layout(area);
        let border = hatch_layout::settled_layout(area, strip, name).dock_border;
        let bottom = detail_panel::interior_regions(border).bottom;

        let rows = engine_render::flex(
            bottom,
            FlexStyle {
                direction: Direction::Column,
                justify_content: Justify::Start,
                align_items: Align::Stretch,
                gap: DOCK_ACTION_GAP_DOTS,
            },
            &[
                FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
                FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            ],
        );
        let [keep, discard] = rows[..] else {
            unreachable!("flex() with 2 children returns exactly 2 rects")
        };
        Some((keep.to_cell_rect(), discard.to_cell_rect()))
    }

    /// Routes input to the offer/picker buttons. Never returns a
    /// `Transition` — the post-hatch action stays in-scene.
    pub(super) fn handle_post_hatch_input(&mut self, ev: InputEvent) -> Option<Transition> {
        let InputEvent::Mouse(me) = ev else { return None };

        let decision = match self.roster_action.as_ref()? {
            RosterAction::Offer { keep, discard } => {
                let keep_clicked = keep.borrow_mut().handle_mouse(&me);
                let discard_clicked = discard.borrow_mut().handle_mouse(&me);
                if keep_clicked {
                    Some(PostHatchDecision::KeepClicked)
                } else if discard_clicked {
                    Some(PostHatchDecision::DiscardClicked)
                } else {
                    None
                }
            }
            RosterAction::Picking { buttons, .. } => {
                let mut clicked = None;
                for (i, core) in buttons.borrow_mut().iter_mut().enumerate() {
                    if core.handle_mouse(&me) {
                        clicked = Some(i);
                    }
                }
                clicked.map(PostHatchDecision::Picked)
            }
        };

        match decision {
            Some(PostHatchDecision::KeepClicked) => self.on_keep_clicked(),
            Some(PostHatchDecision::DiscardClicked) => self.on_discard_clicked(),
            Some(PostHatchDecision::Picked(index)) => self.on_bump_picked(index),
            None => {}
        }
        None
    }

    /// The completed hatch's hatchling, cloned from `self.eggs`, or `None`
    /// without a hatch/egg/hatchling.
    fn completed_hatchling(&self) -> Option<PersistedCreature> {
        let h = self.hatch.as_ref()?;
        self.eggs.get(h.egg)?.hatchling.clone()
    }

    /// The single post-hatch teardown: clears the active hatch, retires the
    /// hatched egg (removed from the tray, `art_cache`, `egg_buttons`, and
    /// `clip_jobs` in lockstep, then persisted), and clears the post-hatch
    /// action and any stale focus. With eggs still remaining, selects the
    /// nearest one (the removed index clamped into the shrunk tray) and
    /// returns to the base browse tray immediately. With no eggs left,
    /// stashes the just-hatched creature into `settled` instead, so the
    /// empty-dock view takes over on the next render.
    fn dismiss_hatch(&mut self) {
        let mut hatchling = None;
        let mut removed_index = None;
        if let Some(h) = self.hatch.take() {
            let removed = h.egg;
            if removed < self.eggs.len() {
                hatchling = self.eggs[removed].hatchling.clone();
                self.eggs.remove(removed);
                self.art_cache.remove(removed);
                self.egg_buttons.get_mut().remove(removed);
                self.remove_egg_from_clip_jobs(removed);
                removed_index = Some(removed);
            }
            self.persist_eggs();
        }
        self.roster_action = None;
        self.mode = super::selection::HatcheryMode::Browsing { hover: 0 };
        match removed_index {
            Some(_) if self.eggs.is_empty() => {
                self.selected = None;
                self.settled = hatchling.map(|creature| super::hatch_render::SettledCreature {
                    idle: super::hatch_render::resolve_idle(&creature),
                    creature,
                });
            }
            Some(removed) => {
                self.settled = None;
                self.select(removed.min(self.eggs.len() - 1));
            }
            None => {
                self.selected = None;
                self.settled = None;
            }
        }
    }

    /// Loads the on-disk roster; with an open slot, appends the hatchling
    /// directly and dismisses the hatch; otherwise shows the bump picker. A
    /// no-op dismissal without a store or a completed hatchling.
    fn on_keep_clicked(&mut self) {
        let Some(hatchling) = self.completed_hatchling() else {
            self.dismiss_hatch();
            return;
        };
        let Some(store) = self.store.as_ref() else {
            self.dismiss_hatch();
            return;
        };

        enum Outcome {
            Placed,
            Picking(Vec<PersistedCreature>),
        }

        let outcome = {
            let mut data = store.load(Self::egg_seed).into_data();
            if data.roster_has_open_slot() {
                data.push_roster(hatchling);
                if let Err(e) = store.save(&data) {
                    tracing::warn!("failed to persist roster addition: {e}");
                }
                Outcome::Placed
            } else {
                Outcome::Picking(data.roster)
            }
        };

        match outcome {
            Outcome::Placed => self.dismiss_hatch(),
            Outcome::Picking(roster) => self.roster_action = Some(RosterAction::picking(roster)),
        }
    }

    /// Discards the hatchling permanently: adds nothing to the roster and
    /// retires the egg the same way a placed hatch does.
    fn on_discard_clicked(&mut self) {
        self.dismiss_hatch();
    }

    /// Bumps the picked candidate for the completed hatchling on disk and
    /// dismisses the hatch. A no-op dismissal without a store or a completed
    /// hatchling.
    fn on_bump_picked(&mut self, index: usize) {
        let Some(hatchling) = self.completed_hatchling() else {
            self.dismiss_hatch();
            return;
        };
        let Some(store) = self.store.as_ref() else {
            self.dismiss_hatch();
            return;
        };

        let mut data = store.load(Self::egg_seed).into_data();
        if let Some(bumped) = data.replace_roster_slot(index, hatchling) {
            dispose_bumped(bumped);
        }
        if let Err(e) = store.save(&data) {
            tracing::warn!("failed to persist roster bump: {e}");
        }
        self.dismiss_hatch();
    }

    /// Draws the current post-hatch roster action, if any: the Keep/Discard
    /// buttons in the settled dock's reserved bottom slot, the bump
    /// picker's title + candidate names, or nothing once placed/discarded
    /// or before offered.
    pub(super) fn draw_dock_actions(&self, frame: &mut Frame, area: Rect) {
        let Some(action) = self.roster_action.as_ref() else { return };
        let white = Style::default().fg(Color::Rgb(0xff, 0xff, 0xff));

        match action {
            RosterAction::Offer { keep, discard } => {
                let Some((keep_rect, discard_rect)) = self.dock_action_rects(area) else { return };
                let mut k = keep.borrow_mut();
                k.set_rect(keep_rect);
                k.render(frame.buffer_mut());
                let mut d = discard.borrow_mut();
                d.set_rect(discard_rect);
                d.render(frame.buffer_mut());
            }
            RosterAction::Picking { roster, buttons } => {
                let focus_cell = super::focus::focus_layout(area).0.to_cell_rect();
                let panel = picker_panel_rect(area, focus_cell);
                let title_rect = Rect { x: panel.x, y: panel.y, width: panel.width, height: PICKER_TITLE_H_CELLS };
                engine_render::label(
                    frame.buffer_mut(),
                    title_rect,
                    "Pick a creature to bump",
                    TextAlign::Center,
                    white,
                );

                let mut cores = buttons.borrow_mut();
                for (i, creature) in roster.iter().enumerate() {
                    let rect = picker_button_rect(panel, i);
                    let Some(core) = cores.get_mut(i) else { continue };
                    core.set_rect(rect);
                    let style = match core.state() {
                        ButtonState::Idle => white,
                        ButtonState::Hover | ButtonState::Pressed => {
                            Style::default().fg(Color::Rgb(0xff, 0xbf, 0x00))
                        }
                    };
                    engine_render::label(frame.buffer_mut(), rect, &creature.name, TextAlign::Center, style);
                }
            }
        }
    }
}
