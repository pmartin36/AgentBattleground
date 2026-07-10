use super::*;

impl RosterManager {
    /// Stamina status text colour (b2-t4) — white, matching
    /// `LEVEL_COLOR`.
    const STAMINA_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);
    /// Divisor for converting an `injured_until` remaining `Duration` into
    /// whole days-remaining (b2-t4). Not `stamina::RECOVERY_DURATION`,
    /// which is a recovery span, not a per-day unit.
    const SECS_PER_DAY: u64 = 24 * 60 * 60;

    /// Ability list text colour (b2-t5) — white, matching
    /// `STAMINA_COLOR`/`LEVEL_COLOR` chrome.
    const ABILITY_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);

    /// The stamina status line for `e` (b2-t4): `"Exhausted: {days} days
    /// remain"` when injured (days derived from `injured_until()`), else
    /// `"Stamina: {percent}%"`. Single source of both format strings.
    pub(super) fn stamina_text(e: &crate::stamina::Stamina) -> String {
        match e.injured_until() {
            Some(remaining) => {
                let days = remaining.as_secs().div_ceil(Self::SECS_PER_DAY);
                format!("Exhausted: {days} days remain")
            }
            None => format!("Stamina: {}%", e.percent()),
        }
    }

    /// Draws `creatures[index]`'s stamina status statically into `rect` as
    /// plain text — no `col_offset` (b2-t4: static panel). Text, so it
    /// renders via `engine_render::label`, never the dot pipeline
    /// (CLAUDE.md constraint 4). The caller gates this on `active_slide()`
    /// so the rect is left entirely unpainted during a transition.
    pub(super) fn render_stamina(&self, buf: &mut Buffer, rect: Rect, index: usize) {
        let text = Self::stamina_text(self.creatures[index].stamina());
        engine_render::label(buf, rect, &text, Self::STAMINA_COLOR);
    }

    /// Builds the flat line list for `creatures[index].abilities()` (b2-t5):
    /// per ability, its description, then (if it has any) its modifier names
    /// joined with ", " on their own line. Single source of the line format.
    pub(super) fn ability_lines(&self, index: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for ability in self.creatures[index].abilities() {
            lines.push(ability.description().to_string());
            if !ability.modifiers().is_empty() {
                let names = ability
                    .modifiers()
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(names);
            }
        }
        lines
    }

    /// Draws `creatures[index]`'s full ability list statically into `rect` as
    /// plain text — one line per row, top-down, no wrapping/scrolling
    /// (overflow degrades via `label`'s silent truncation; see research.md).
    /// Text, so it renders via `engine_render::label`, never the dot pipeline
    /// (CLAUDE.md constraint 4). The caller gates this on `active_slide()` so
    /// the rect is left entirely unpainted during a transition.
    pub(super) fn render_ability_list(&self, buf: &mut Buffer, rect: Rect, index: usize) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        for (i, line) in self.ability_lines(index).into_iter().enumerate() {
            let y = rect.y + i as u16;
            if y >= rect.bottom() {
                break;
            }
            engine_render::label(buf, Rect::new(rect.x, y, rect.width, 1), &line, Self::ABILITY_COLOR);
        }
    }

}

/// b1-t2: stamina/injury status text (upper-right), disappearing entirely
/// during an active slide (no cross-fade), reappearing populated with the
/// incoming creature's data once settled.
#[cfg(test)]
mod stamina_render_tests {
    use super::*;
    use crate::creatures::Creature;
    use crate::scenes::test_util::{key_event, rect_text, render_to_buffer};
    use crate::stamina::Stamina;
    use crossterm::event::KeyCode;
    use engine_core::scene::EngineCtx;

    /// A rested creature (non-injured, real percent) renders
    /// `"Stamina: {N}%"`, never a "days remain" form.
    #[test]
    fn stamina_shows_percent_for_rested_creature() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test")
            .with_stamina(Stamina::default().drain_from_damage(58));
        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::stamina_rect(area);

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Stamina: 42%"),
            "expected \"Stamina: 42%\" for a rested creature; got {text:?}"
        );
        assert!(
            !text.contains("days remain"),
            "a rested (non-injured) creature must not show the days-remain form; got {text:?}"
        );
    }

    /// An injured creature (`injured_until` set) renders
    /// `"Exhausted: {N} days remain"` instead of a percent, N derived from
    /// `injured_until()` (24h recovery -> 1 day). Unchanged by the b1-t2
    /// rename (spec keeps the "Exhausted" display word).
    #[test]
    fn stamina_shows_days_remain_for_injured_creature() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test")
            .with_stamina(Stamina::default().drain_from_damage(100));
        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::stamina_rect(area);

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Exhausted: 1 days remain"),
            "expected \"Exhausted: 1 days remain\" for an injured creature; got {text:?}"
        );
        assert!(
            !text.contains('%'),
            "an injured creature must not show a percent form; got {text:?}"
        );
    }

    /// During an active slide (at trigger and at partial progress), the
    /// stamina status is entirely absent — no cross-fade. Asserted as the
    /// absence of any status TEXT (ASCII alphanumerics), which tolerates the
    /// sliding sprite that legitimately crosses this region: the sprite renders
    /// as braille dots (never ASCII), so an alphanumeric character in the rect
    /// can only be stamina text.
    #[test]
    fn stamina_hidden_during_slide() {
        let (w, h) = (80u16, 30u16);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::stamina_rect(area);
        let has_text = |buf: &ratatui::buffer::Buffer| {
            rect_text(buf, rect).chars().any(|c| c.is_ascii_alphanumeric())
        };

        let mut scene = RosterManager::new();
        scene.handle_input(key_event(KeyCode::Right)); // trigger slide, no update() yet
        let trigger_buf = render_to_buffer(&scene, w, h);
        assert!(
            !has_text(&trigger_buf),
            "stamina rect must show no status text at the instant a slide triggers"
        );

        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(75)); // ~25% of the 300ms SLIDE_DUR
        let mid_buf = render_to_buffer(&scene, w, h);
        assert!(
            !has_text(&mid_buf),
            "stamina rect must show no status text mid-slide"
        );
    }

    /// Once the slide settles (elapsed past SLIDE_DUR), the stamina rect
    /// shows the INCOMING (new current_index) creature's data, not the
    /// outgoing creature's.
    #[test]
    fn stamina_shows_incoming_after_settle() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Rested")
            .with_stamina(Stamina::default().drain_from_damage(58));
        rm.creatures[1] = Creature::new("Injured")
            .with_stamina(Stamina::default().drain_from_damage(100));

        let mut ctx = EngineCtx;
        rm.handle_input(key_event(KeyCode::Right)); // nav 0 -> 1
        rm.update(&mut ctx, Duration::from_millis(350)); // past the 300ms SLIDE_DUR

        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::stamina_rect(area);

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Exhausted: 1 days remain"),
            "settled render must show the incoming (index 1, injured) creature's data; got {text:?}"
        );
        assert!(
            !text.contains("Stamina: 42%"),
            "settled render must not still show the outgoing (index 0) creature's data; got {text:?}"
        );
    }
}

/// b2-t5: full ability list (right side, below stamina), disappearing
/// entirely during an active slide (no cross-fade), reappearing populated
/// with the incoming creature's abilities once settled.
#[cfg(test)]
mod ability_list_render_tests {
    use super::*;
    use crate::ability::{Ability, Modifier};
    use crate::creatures::Creature;
    use crate::scenes::test_util::{key_event, rect_text, render_to_buffer};
    use crossterm::event::KeyCode;
    use engine_core::scene::EngineCtx;

    /// Every ability's description AND every modifier's name must appear
    /// simultaneously inside `ability_list` — full expansion, no progressive
    /// disclosure/pagination.
    #[test]
    fn ability_list_shows_all_abilities_and_modifiers_at_rest() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test").with_abilities(vec![
            Ability::new(
                "Fire Breath",
                vec![Modifier { name: "Burning".into(), requires: None }],
            ),
            Ability::new(
                "Ice Shard",
                vec![Modifier { name: "Frozen".into(), requires: None }],
            ),
        ]);
        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::ability_list_rect(area);

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Fire Breath"),
            "expected \"Fire Breath\" ability description; got {text:?}"
        );
        assert!(
            text.contains("Burning"),
            "expected \"Burning\" modifier name; got {text:?}"
        );
        assert!(
            text.contains("Ice Shard"),
            "expected \"Ice Shard\" ability description; got {text:?}"
        );
        assert!(
            text.contains("Frozen"),
            "expected \"Frozen\" modifier name; got {text:?}"
        );
    }

    /// During an active slide (at trigger and at partial progress), the
    /// ability-list rect shows none of the current creature's ability text —
    /// the display is entirely absent, not cross-faded. Checked via a
    /// distinctive substring rather than whole-rect blankness so the test is
    /// robust regardless of what else may paint in/near the rect.
    #[test]
    fn ability_list_hidden_during_slide() {
        let (w, h) = (80u16, 30u16);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::ability_list_rect(area);

        let mut scene = RosterManager::new();
        scene.creatures[0] =
            Creature::new("Test").with_abilities(vec![Ability::new("Fire Breath", vec![])]);

        let rest_buf = render_to_buffer(&scene, w, h);
        assert!(
            rect_text(&rest_buf, rect).contains("Fire Breath"),
            "sanity check: ability text must render at rest before triggering a slide"
        );

        scene.handle_input(key_event(KeyCode::Right)); // trigger slide, no update() yet
        let trigger_buf = render_to_buffer(&scene, w, h);
        assert!(
            !rect_text(&trigger_buf, rect).contains("Fire Breath"),
            "ability_list must show no ability text at the instant a slide triggers"
        );

        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(75)); // ~25% of the 300ms SLIDE_DUR
        let mid_buf = render_to_buffer(&scene, w, h);
        assert!(
            !rect_text(&mid_buf, rect).contains("Fire Breath"),
            "ability_list must remain free of ability text mid-slide"
        );
    }

    /// Once the slide settles (elapsed past SLIDE_DUR), the ability-list rect
    /// shows the INCOMING (new current_index) creature's abilities, not the
    /// outgoing creature's.
    #[test]
    fn ability_list_shows_incoming_after_settle() {
        let mut rm = RosterManager::new();
        rm.creatures[0] =
            Creature::new("Outgoing").with_abilities(vec![Ability::new("Outgoing Move", vec![])]);
        rm.creatures[1] =
            Creature::new("Incoming").with_abilities(vec![Ability::new("Incoming Move", vec![])]);

        let mut ctx = EngineCtx;
        rm.handle_input(key_event(KeyCode::Right)); // nav 0 -> 1
        rm.update(&mut ctx, Duration::from_millis(350)); // past the 300ms SLIDE_DUR

        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::ability_list_rect(area);

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Incoming Move"),
            "settled render must show the incoming (index 1) creature's ability; got {text:?}"
        );
        assert!(
            !text.contains("Outgoing Move"),
            "settled render must not still show the outgoing (index 0) creature's ability; got {text:?}"
        );
    }
}

