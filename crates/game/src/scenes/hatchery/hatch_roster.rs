//! Post-hatch "Add to Roster" action: offers to place the freshly hatched
//! creature into the roster once the hatch sequence completes, either
//! directly (an open slot) or via a pick-a-creature-to-bump step when the
//! roster is full. The pick step and the bumped creature's disposal
//! (`dispose_bumped`) stay two distinct steps so the disposal can later be
//! swapped for a move-to-Farm/Playpen action without reworking the pick
//! flow.

use std::cell::RefCell;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::Frame;

use engine_core::scene::{InputEvent, Transition};
use engine_render::{Button, ButtonCore, ButtonState, TextAlign};

use crate::player_data::PersistedCreature;

/// What the post-hatch UI shows, one state at a time.
pub(super) enum RosterAction {
    /// The hatch has completed; the "Add to Roster" button is shown.
    Offer { button: RefCell<Button> },
    /// The roster was full; the player is picking a creature to bump.
    Picking { roster: Vec<PersistedCreature>, buttons: RefCell<Vec<ButtonCore>> },
}

impl RosterAction {
    /// A fresh offer, mirroring the define-modal Done button's construction.
    fn offer() -> Self {
        RosterAction::Offer {
            button: RefCell::new(
                Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Add to Roster"),
            ),
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

/// Width, in cells, of the "Add to Roster" button and each picker candidate
/// row.
const BUTTON_W_CELLS: u16 = 18;
/// Height, in cells, of the "Add to Roster" button.
const ADD_BUTTON_H_CELLS: u16 = 3;
/// Gap, in cells, between the reveal's bottom edge and the button/panel
/// below it.
const GAP_CELLS: u16 = 1;
/// Height, in cells, of the picker's title row.
const PICKER_TITLE_H_CELLS: u16 = 1;
/// Height, in cells, of each picker candidate row.
const PICKER_ROW_H_CELLS: u16 = 2;

/// Rect for the "Add to Roster" button, centered below the reveal.
pub(super) fn add_button_rect(focus_cell: Rect) -> Rect {
    let cx = focus_cell.x + focus_cell.width / 2;
    let x = cx.saturating_sub(BUTTON_W_CELLS / 2);
    let y = focus_cell.y + focus_cell.height + GAP_CELLS;
    Rect { x, y, width: BUTTON_W_CELLS, height: ADD_BUTTON_H_CELLS }
}

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
    AddClicked,
    Picked(usize),
}

impl super::Hatchery {
    /// Once the active hatch completes, offers the "Add to Roster" action.
    /// A no-op once an action has already been set, or while no hatch is
    /// active/complete.
    pub(super) fn maybe_offer_add_to_roster(&mut self) {
        let complete = self.hatch.as_ref().is_some_and(|h| h.seq.is_complete());
        if complete && self.roster_action.is_none() {
            self.roster_action = Some(RosterAction::offer());
        }
    }

    /// Routes input to the offer/picker buttons. Never returns a
    /// `Transition` — add-to-roster stays in-scene.
    pub(super) fn handle_post_hatch_input(&mut self, ev: InputEvent) -> Option<Transition> {
        let InputEvent::Mouse(me) = ev else { return None };

        let decision = match self.roster_action.as_ref()? {
            RosterAction::Offer { button } => {
                button.borrow_mut().handle_mouse(&me).then_some(PostHatchDecision::AddClicked)
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
            Some(PostHatchDecision::AddClicked) => self.on_add_to_roster_clicked(),
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
    /// action and any stale focus — leaving the scene back at the base tray
    /// with the back button and egg slots reachable again.
    fn dismiss_hatch(&mut self) {
        if let Some(h) = self.hatch.take() {
            if h.egg < self.eggs.len() {
                self.eggs.remove(h.egg);
                self.art_cache.remove(h.egg);
                self.egg_buttons.get_mut().remove(h.egg);
                self.remove_egg_from_clip_jobs(h.egg);
            }
            self.persist_eggs();
        }
        self.roster_action = None;
        self.focused = None;
    }

    /// Loads the on-disk roster; with an open slot, appends the hatchling
    /// directly and dismisses the hatch; otherwise shows the bump picker. A
    /// no-op dismissal without a store or a completed hatchling.
    fn on_add_to_roster_clicked(&mut self) {
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

    /// Draws the current post-hatch roster action, if any: the offer button
    /// below the reveal, the bump picker's title + candidate names, or
    /// nothing once placed / before offered.
    pub(super) fn draw_add_to_roster(&self, frame: &mut Frame, area: Rect) {
        let Some(action) = self.roster_action.as_ref() else { return };
        let focus_cell = super::focus::focus_layout(area).0.to_cell_rect();
        let white = Style::default().fg(Color::Rgb(0xff, 0xff, 0xff));

        match action {
            RosterAction::Offer { button } => {
                let rect = add_button_rect(focus_cell);
                let mut b = button.borrow_mut();
                b.set_rect(rect);
                b.render(frame.buffer_mut());
            }
            RosterAction::Picking { roster, buttons } => {
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
