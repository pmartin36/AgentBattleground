use super::*;

/// The hand-authored demo playback sequence (b3-t1): Team A piece index 0
/// advances into the board while Team B piece index 6 dies, their windows
/// partially overlapping and sharing a `turn`, per spec 05's "hand-authored/
/// hardcoded directly in the scene" decision. Every `start_time` is `> 0.0`
/// so the elapsed==0.0 baseline is unperturbed.
pub fn demo_events() -> Vec<Event> {
    vec![
        // Team A piece (index 0) advances into the board.
        Event {
            turn: 1,
            start_time: 1.0,
            duration: 1.2,
            kind: EventKind::Move {
                piece_index: 0,
                to: (3, 3),
            },
        },
        // Team B piece (index 6) dies; window [1.6,2.6) overlaps the move's [1.0,2.2).
        Event {
            turn: 1,
            start_time: 1.6,
            duration: 1.0,
            kind: EventKind::Die { piece_index: 6 },
        },
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// b4-t3: per-piece render pipeline — team tint, mirror, phase-staggered idle
// frame. Signatures/constants per research.md blueprint; bodies are stubs for
// the code-writer (test-writer only pins the observable contract).
// ─────────────────────────────────────────────────────────────────────────────

/// A single playback event: a `Move` or `Die` acting on one piece, active
/// during `[start_time, start_time + duration)`. `turn` is a separate,
/// discrete grouping tag: multiple events may share the same `turn` while
/// having different `start_time`s — `turn` does not replace the clock, it
/// only labels which turn produced each event.
///
/// `EventKind::Move` carries only a destination (`to`), never a `from` — the
/// glide interpolates from wherever the piece's `Transform.translate` (or,
/// for `Die`, `Transform.scale`) actually is when the event's window opens,
/// via the existing `Tween`/`ease_in_out` utility. Remembering that starting
/// value for the duration of a multi-frame tween is transient, scene-internal
/// runtime bookkeeping (e.g. a small cache populated the frame an event's
/// window begins), not part of the authored `Event` data — the same way
/// `18-battle-viewer-baseline` keeps per-frame render state separate from the
/// data it derives from. This bookkeeping cache lives on `BattleViewer`
/// (added in b2-t1), not on `Event` itself.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Event {
    pub turn: u32,
    pub start_time: f32,
    pub duration: f32,
    pub kind: EventKind,
}

/// The kind of playback event. `piece_index` targets `Piece.index` — resolve
/// via `.iter()`/`.iter_mut().find(|p| p.index == piece_index)`, never
/// `pieces[piece_index]` — independent of `Piece.team`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EventKind {
    /// Moves the piece to `to`. Carries only a destination, no `from` field
    /// (MUST NOT gain one — the from-value is transient runtime bookkeeping,
    /// not authored data; see the doc comment on `Event` above).
    Move { piece_index: usize, to: (u16, u16) },
    /// Marks the piece dead.
    Die { piece_index: usize },
}

impl BattleViewer {
    /// Drives every event whose window has begun and is not yet settled,
    /// every frame — independent of any other event, per the spec's overlap
    /// rule ("the playback clock evaluates which events are active at the
    /// current elapsed time every frame and drives every affected piece
    /// simultaneously"). Handles both `Move` (b2-t2) and `Die` (b2-t3) via
    /// the same loop shape.
    pub(super) fn drive_events(&mut self) {
        for event_index in 0..self.events.len() {
            let event = self.events[event_index];
            if self.elapsed < event.start_time || self.settled_events.contains(&event_index) {
                continue;
            }
            match event.kind {
                EventKind::Move { piece_index, to } => {
                    let Some(piece) = self.pieces.iter_mut().find(|p| p.index == piece_index)
                    else {
                        continue;
                    };

                    // Gameplay truth commits instantly the frame the window opens;
                    // capture the from-value for the cosmetic tween in the same
                    // instant (transient bookkeeping, not authored `Event` data).
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        self.event_from_values.entry(piece_index)
                    {
                        entry.insert((piece.transform.translate.x, piece.transform.translate.y));
                        piece.col = to.0;
                        piece.row = to.1;
                    }

                    let target = world_pos_for_cell(to.0, to.1);
                    let end_time = event.start_time + event.duration;
                    if self.elapsed >= end_time {
                        // Exact landing — no residual Tween float drift — and settle
                        // once so a later external edit is never re-fought.
                        piece.transform.translate = target;
                        self.event_from_values.remove(&piece_index);
                        self.settled_events.insert(event_index);
                    } else {
                        let (from_x, from_y) = self.event_from_values[&piece_index];
                        let since_start =
                            Duration::from_secs_f32((self.elapsed - event.start_time).max(0.0));
                        let dur = Duration::from_secs_f32(event.duration);
                        let x = Tween::new(from_x, target.x, dur).at(since_start);
                        let y = Tween::new(from_y, target.y, dur).at(since_start);
                        piece.transform.translate = WorldPos::new(x, y);
                    }
                }
                EventKind::Die { piece_index } => {
                    let Some(piece) = self.pieces.iter_mut().find(|p| p.index == piece_index)
                    else {
                        continue;
                    };

                    // First frame the window is open: capture starting scale
                    // (no col/row commit — Die does not move the piece).
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        self.event_from_values.entry(piece_index)
                    {
                        entry.insert((piece.transform.scale.x, piece.transform.scale.y));
                    }

                    let end_time = event.start_time + event.duration;
                    if self.elapsed >= end_time {
                        // Exact landing — no residual Tween float drift — and settle
                        // once so a later external edit (e.g. a revive) is never
                        // re-fought.
                        piece.transform.scale = Vec2::splat(0.0);
                        piece.alive = false;
                        self.event_from_values.remove(&piece_index);
                        self.settled_events.insert(event_index);
                    } else {
                        let (from_x, from_y) = self.event_from_values[&piece_index];
                        let since_start =
                            Duration::from_secs_f32((self.elapsed - event.start_time).max(0.0));
                        let dur = Duration::from_secs_f32(event.duration);
                        let x = Tween::new(from_x, 0.0, dur).at(since_start);
                        let y = Tween::new(from_y, 0.0, dur).at(since_start);
                        piece.transform.scale = Vec2::new(x, y);
                    }
                }
            }
        }
    }

}

#[cfg(test)]
mod event_data_model_tests {
    use super::*;

    /// SUGGESTED_TESTS: every field of a `Move` and a `Die` `Event` round-trips.
    #[test]
    fn event_move_and_die_fields_round_trip() {
        let mv = Event {
            turn: 3,
            start_time: 1.5,
            duration: 0.4,
            kind: EventKind::Move {
                piece_index: 2,
                to: (5, 6),
            },
        };
        assert_eq!(mv.turn, 3);
        assert_eq!(mv.start_time, 1.5);
        assert_eq!(mv.duration, 0.4);
        match mv.kind {
            EventKind::Move { piece_index, to } => {
                assert_eq!(piece_index, 2);
                assert_eq!(to, (5, 6));
            }
            _ => panic!("expected EventKind::Move"),
        }

        let die = Event {
            turn: 7,
            start_time: 2.0,
            duration: 0.8,
            kind: EventKind::Die { piece_index: 9 },
        };
        assert_eq!(die.turn, 7);
        assert_eq!(die.start_time, 2.0);
        assert_eq!(die.duration, 0.8);
        match die.kind {
            EventKind::Die { piece_index } => assert_eq!(piece_index, 9),
            _ => panic!("expected EventKind::Die"),
        }
    }

    /// `turn` is a separate grouping tag, independent of `start_time`: two
    /// events sharing the same `turn` but different `start_time`s must both
    /// be preserved independently (proves `turn` doesn't collapse/alias with
    /// `start_time`).
    #[test]
    fn turn_does_not_alias_start_time() {
        let e1 = Event {
            turn: 4,
            start_time: 0.1,
            duration: 0.2,
            kind: EventKind::Die { piece_index: 0 },
        };
        let e2 = Event {
            turn: 4,
            start_time: 0.9,
            duration: 0.2,
            kind: EventKind::Die { piece_index: 1 },
        };
        assert_eq!(e1.turn, e2.turn, "both events share the same turn");
        assert_ne!(
            e1.start_time, e2.start_time,
            "start_time is independent of turn and must not be aliased"
        );
    }

    /// Compile-time guard: `EventKind::Move` has EXACTLY `piece_index` and
    /// `to` — no `from` field. An exhaustive struct-pattern destructure (no
    /// `..`) fails to COMPILE the moment an extra field (e.g. `from`) is
    /// added to the variant, per the spec's explicit "MUST NOT gain a `from`
    /// field."
    #[test]
    fn move_variant_has_exactly_piece_index_and_to_no_from() {
        let kind = EventKind::Move {
            piece_index: 0,
            to: (0, 0),
        };
        let EventKind::Move { piece_index, to } = kind else {
            panic!("expected EventKind::Move");
        };
        assert_eq!(piece_index, 0);
        assert_eq!(to, (0, 0));
    }

    /// A doc comment documenting the transient from-value bookkeeping
    /// mechanism (populated the frame an event's window begins, scene-
    /// internal runtime state, not part of the authored `Event` data) must be
    /// present near the `Event`/`EventKind` declarations — grep-verifiable.
    #[test]
    fn doc_comment_documents_transient_from_value_bookkeeping() {
        // Event/EventKind now live in this file (playback.rs) post-split; the
        // transient-bookkeeping doc must sit before the Event declaration.
        // Checking only the slice before `pub struct Event` keeps this from
        // vacuously matching this test module's own doc text further down.
        let src = include_str!("playback.rs");
        let event_decl = src
            .find("pub struct Event")
            .expect("Event struct must exist in this file");
        let section = &src[..event_decl];
        let lower = section.to_lowercase();
        assert!(
            lower.contains("transient") && lower.contains("bookkeeping"),
            "a doc comment on Event/EventKind must document the transient \
             from-value bookkeeping mechanism (expected the words \
             'transient' and 'bookkeeping' somewhere before BattleViewer)"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: BattleViewer.events / event_from_values wiring (b2-t1) — fields
// exist and default empty; update()/render() are not yet touched.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod event_playback_wiring_tests {
    use super::*;

    /// DELIVERABLE (b3-t1): `BattleViewer::default()` carries a hand-authored
    /// demo event sequence — at least one `Move` and one `Die` — per the
    /// spec's "hand-authored/hardcoded directly in the scene" decision.
    #[test]
    fn default_events_contains_a_move_and_a_die() {
        let scene = BattleViewer::default();
        let has_move = scene
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Move { .. }));
        let has_die = scene
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Die { .. }));
        assert!(
            has_move,
            "default().events must contain at least one Move event, got {:?}",
            scene.events
        );
        assert!(
            has_die,
            "default().events must contain at least one Die event, got {:?}",
            scene.events
        );
    }

    /// DELIVERABLE: no authored event's `start_time` may be `<= 0.0` — this
    /// protects every elapsed==0.0 baseline test from perturbation now that
    /// real demo content is wired in.
    #[test]
    fn default_events_all_start_after_zero() {
        let scene = BattleViewer::default();
        assert!(
            !scene.events.is_empty(),
            "default().events must be non-empty once the demo sequence is authored"
        );
        for (i, e) in scene.events.iter().enumerate() {
            assert!(
                e.start_time > 0.0,
                "event[{i}] start_time must be > 0.0 to preserve the elapsed==0.0 baseline, \
                 got {}",
                e.start_time
            );
        }
    }

    /// DELIVERABLE: the authored Move and Die windows partially overlap,
    /// exercising the shipped overlap-handling path (b2-t5) in real demo
    /// data, per this task's own "at least partially overlapping" instruction.
    #[test]
    fn default_events_move_and_die_windows_partially_overlap() {
        let scene = BattleViewer::default();
        let move_event = scene
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Move { .. }))
            .expect("a Move event must be present");
        let die_event = scene
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Die { .. }))
            .expect("a Die event must be present");

        let move_end = move_event.start_time + move_event.duration;
        let die_end = die_event.start_time + die_event.duration;
        let overlap_start = move_event.start_time.max(die_event.start_time);
        let overlap_end = move_end.min(die_end);
        assert!(
            overlap_start < overlap_end,
            "Move [{}, {}) and Die [{}, {}) windows must partially overlap",
            move_event.start_time,
            move_end,
            die_event.start_time,
            die_end
        );
    }

    /// DELIVERABLE: multiple events may legitimately share a `turn` while
    /// having different `start_time`s — the authored Move and Die share a
    /// `turn` tag, demonstrating `turn` does not replace the clock.
    #[test]
    fn default_events_move_and_die_share_a_turn() {
        let scene = BattleViewer::default();
        let move_turn = scene
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Move { .. }))
            .expect("a Move event must be present")
            .turn;
        let die_turn = scene
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Die { .. }))
            .expect("a Die event must be present")
            .turn;
        assert_eq!(
            move_turn, die_turn,
            "authored Move and Die events should share a turn tag while differing in \
             start_time"
        );
    }

    /// DELIVERABLE: the transient from-value bookkeeping cache (documented on
    /// `Event` above) starts empty — nothing has captured a starting
    /// translate/scale before any event has begun driving.
    #[test]
    fn default_event_from_values_is_empty() {
        let scene = BattleViewer::default();
        assert!(
            scene.event_from_values.is_empty(),
            "BattleViewer::default().event_from_values must start empty, got {:?}",
            scene.event_from_values
        );
    }

    /// b4-t1 regression guard: every `demo_events()` `piece_index` must
    /// resolve to a real `Piece` under `pieces()`'s current numbering — a
    /// future roster resize must not silently strand a demo event pointing
    /// at a piece that no longer exists. Uses the module's `.find`/`.any(|p|
    /// p.index == ..)` resolution idiom (never positional `pieces[i]`
    /// indexing, per the module doc's stable-index convention).
    #[test]
    fn default_events_piece_indices_resolve_to_real_pieces() {
        let ps = pieces();
        let scene = BattleViewer::default();
        for e in &scene.events {
            let pi = match e.kind {
                EventKind::Move { piece_index, .. } => piece_index,
                EventKind::Die { piece_index } => piece_index,
            };
            assert!(
                ps.iter().any(|p| p.index == pi),
                "event {:?} references piece_index {pi}, which does not resolve to any \
                 Piece in pieces() (valid indices: {:?})",
                e,
                ps.iter().map(|p| p.index).collect::<Vec<_>>()
            );
        }
    }

    /// b4-t1: pins the demo's intended semantics — the authored Move targets
    /// a Team A piece, the authored Die targets a Team B piece — derived from
    /// `pieces()`, not bare literals.
    #[test]
    fn default_events_move_targets_team_a_and_die_targets_team_b() {
        let ps = pieces();
        let scene = BattleViewer::default();
        let move_index = scene
            .events
            .iter()
            .find_map(|e| match e.kind {
                EventKind::Move { piece_index, .. } => Some(piece_index),
                _ => None,
            })
            .expect("a Move event must be present");
        let die_index = scene
            .events
            .iter()
            .find_map(|e| match e.kind {
                EventKind::Die { piece_index } => Some(piece_index),
                _ => None,
            })
            .expect("a Die event must be present");

        let move_piece = ps
            .iter()
            .find(|p| p.index == move_index)
            .expect("Move's piece_index must resolve to a real piece");
        let die_piece = ps
            .iter()
            .find(|p| p.index == die_index)
            .expect("Die's piece_index must resolve to a real piece");

        assert_eq!(
            move_piece.team,
            Team::A,
            "authored Move should target a Team A piece, got {:?}",
            move_piece.team
        );
        assert_eq!(
            die_piece.team,
            Team::B,
            "authored Die should target a Team B piece, got {:?}",
            die_piece.team
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: `Move` event driving in `update()` (b2-t2) — instant col/row commit,
// cosmetic transform.translate lerp via Tween/ease_in_out, exact landing,
// settle-once (does not re-fight an externally mutated transform.translate
// after the event has completed).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod move_event_driving_tests {
    use super::*;
    use engine_core::scene::EngineCtx;

    /// Builds a fresh default scene with piece 0's only playback event: a
    /// `Move` from its seeded start cell to `to`, active on
    /// `[start_time, start_time + duration)`.
    fn scene_with_single_move(start_time: f32, duration: f32, to: (u16, u16)) -> BattleViewer {
        BattleViewer {
            events: vec![Event {
                turn: 1,
                start_time,
                duration,
                kind: EventKind::Move {
                    piece_index: 0,
                    to,
                },
            }],
            ..BattleViewer::default()
        }
    }

    /// DELIVERABLE (3): `col`/`row` commit to `to` in the SAME instant the
    /// event's window opens (`elapsed >= start_time`), not before.
    #[test]
    fn move_col_row_commits_instantly_at_window_open_not_before() {
        let mut ctx = EngineCtx;

        let mut before = scene_with_single_move(1.0, 1.0, (5, 0));
        before.update(&mut ctx, Duration::from_secs_f32(0.999));
        assert_eq!(
            (before.pieces[0].col, before.pieces[0].row),
            (ACTIVE_COLS[0], TEAM_A_ROW),
            "col/row must NOT yet commit to `to` before start_time"
        );

        let mut at_open = scene_with_single_move(1.0, 1.0, (5, 0));
        at_open.update(&mut ctx, Duration::from_secs_f32(1.0));
        assert_eq!(
            (at_open.pieces[0].col, at_open.pieces[0].row),
            (5, 0),
            "col/row must commit to `to` the instant elapsed reaches start_time"
        );
    }

    /// DELIVERABLE (1): strictly between `start_time` and
    /// `start_time + duration`, `transform.translate` is mid-glide — neither
    /// the old cell's world position nor the new cell's world position.
    #[test]
    fn move_transform_translate_strictly_between_endpoints_mid_tween() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_single_move(1.0, 1.0, (5, TEAM_A_ROW));
        let from = world_pos_for_cell(ACTIVE_COLS[0], TEAM_A_ROW);
        let to = world_pos_for_cell(5, TEAM_A_ROW);

        scene.update(&mut ctx, Duration::from_secs_f32(1.5));
        let mid = scene.pieces[0].transform.translate;

        assert_eq!(mid.y, from.y, "row is unchanged, y must not move");
        assert!(
            mid.x > from.x.min(to.x) && mid.x < from.x.max(to.x),
            "mid-tween translate.x ({}) must be strictly between the start ({}) and end ({}) x",
            mid.x,
            from.x,
            to.x
        );
    }

    /// DELIVERABLE (2): at/after `start_time + duration`, `transform.translate`
    /// lands EXACTLY on `world_pos_for_cell(to)` — no residual Tween float
    /// drift.
    #[test]
    fn move_transform_translate_lands_exactly_at_target_after_duration() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_single_move(1.0, 1.0, (5, 0));

        scene.update(&mut ctx, Duration::from_secs_f32(2.0));

        assert_eq!(
            scene.pieces[0].transform.translate,
            world_pos_for_cell(5, 0),
            "transform.translate must land exactly on the target cell's center once the \
             event's window has fully elapsed"
        );
    }

    /// DELIVERABLE (4) settle regression: once the `Move` event has fully
    /// completed, an externally-mutated `transform.translate` (e.g. an
    /// inspector edit) must NOT be re-derived/overwritten by a later
    /// `update()` call for the same already-settled event.
    #[test]
    fn move_settled_event_does_not_refight_externally_mutated_translate() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_single_move(1.0, 1.0, (5, 0));

        // Complete the event.
        scene.update(&mut ctx, Duration::from_secs_f32(2.0));
        assert_eq!(
            scene.pieces[0].transform.translate,
            world_pos_for_cell(5, 0),
            "test setup: event must have landed exactly before the external-edit step"
        );

        // Simulate an external (e.g. inspector) edit after settling.
        let external = WorldPos::new(9.25, 9.25);
        scene.pieces[0].transform.translate = external;

        // Further updates must not touch the already-settled event's piece.
        scene.update(&mut ctx, Duration::from_secs_f32(1.0));
        assert_eq!(
            scene.pieces[0].transform.translate, external,
            "an already-settled Move event must not overwrite a later external edit to \
             transform.translate"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: Die event driving in update() (b2-t3) — scale-to-zero lerp, `alive`
// flip, settle-once (does not re-fight an externally revived `alive` after
// the event has completed).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod die_event_driving_tests {
    use super::*;
    use engine_core::scene::EngineCtx;

    /// Builds a fresh default scene with piece 0's only playback event: a
    /// `Die`, active on `[start_time, start_time + duration)`.
    fn scene_with_single_die(start_time: f32, duration: f32) -> BattleViewer {
        BattleViewer {
            events: vec![Event {
                turn: 1,
                start_time,
                duration,
                kind: EventKind::Die { piece_index: 0 },
            }],
            ..BattleViewer::default()
        }
    }

    /// DELIVERABLE (1): sampling `transform.scale`'s magnitude at several
    /// strictly-increasing elapsed times within the event's active window
    /// shows strictly decreasing magnitude (progressive shrink, not a jump to
    /// zero).
    #[test]
    fn die_scale_magnitude_strictly_decreases_within_window() {
        let mut ctx = EngineCtx;

        let mut prev_mag = f32::MAX;
        for t in [1.25_f32, 1.5, 1.75] {
            // Fresh scene per sample: the event is re-driven from t=0 each
            // time so each sample reflects only elapsed time `t`, not
            // accumulated per-frame drift.
            let mut probe = scene_with_single_die(1.0, 1.0);
            probe.update(&mut ctx, Duration::from_secs_f32(t));
            let s = probe.pieces[0].transform.scale;
            let mag = (s.x * s.x + s.y * s.y).sqrt();
            assert!(
                mag < prev_mag,
                "scale magnitude at t={t} ({mag}) must be strictly less than the previous \
                 sample ({prev_mag})"
            );
            prev_mag = mag;
        }
    }

    /// DELIVERABLE (2): `alive` is `true` up to just before
    /// `start_time + duration`, and exactly `false` (with `scale` snapped to
    /// zero) at/after it.
    #[test]
    fn die_alive_flips_false_exactly_at_completion() {
        let mut ctx = EngineCtx;

        let mut before = scene_with_single_die(1.0, 1.0);
        before.update(&mut ctx, Duration::from_secs_f32(1.999));
        assert!(
            before.pieces[0].alive,
            "alive must still be true strictly before start_time + duration"
        );

        let mut at_complete = scene_with_single_die(1.0, 1.0);
        at_complete.update(&mut ctx, Duration::from_secs_f32(2.0));
        assert!(
            !at_complete.pieces[0].alive,
            "alive must be false the instant elapsed reaches start_time + duration"
        );
        assert_eq!(
            at_complete.pieces[0].transform.scale,
            Vec2::splat(0.0),
            "transform.scale must land exactly on zero once the event's window has fully \
             elapsed"
        );
    }

    /// DELIVERABLE (3) settle regression: once the `Die` event has fully
    /// completed (`alive == false`), an externally-revived `alive` (the
    /// spec's named hypothetical revive mechanic) must NOT be re-flipped back
    /// to `false` by a later `update()` call for the same already-settled
    /// event.
    #[test]
    fn die_settled_event_does_not_refight_external_revive() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_single_die(1.0, 1.0);

        // Complete the event.
        scene.update(&mut ctx, Duration::from_secs_f32(2.0));
        assert!(
            !scene.pieces[0].alive,
            "test setup: event must have settled (alive == false) before the revive step"
        );

        // Simulate an external revive after settling.
        scene.pieces[0].alive = true;

        // Further updates must not touch the already-settled event's piece.
        scene.update(&mut ctx, Duration::from_secs_f32(1.0));
        assert!(
            scene.pieces[0].alive,
            "an already-settled Die event must not re-flip an externally revived `alive` \
             back to false"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: overlapping/simultaneous multi-piece events in one frame (b2-t5) —
// proves the spec's "Events may overlap in time... drives every affected
// piece simultaneously" bullet: a single update() landing two different
// events (on two different pieces) mid-flight drives BOTH independently, with
// no leakage between them.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod overlapping_events_tests {
    use super::*;
    use engine_core::scene::EngineCtx;

    /// Builds a scene with two simultaneous, independent events on different
    /// pieces: piece 0 (Team A) `Move`s to `(5, 0)`, piece 6 (Team B) `Die`s —
    /// both active on the same `[start_time, start_time + duration)` window.
    fn scene_with_overlapping_move_and_die(start_time: f32, duration: f32) -> BattleViewer {
        BattleViewer {
            events: vec![
                Event {
                    turn: 1,
                    start_time,
                    duration,
                    kind: EventKind::Move {
                        piece_index: 0,
                        to: (5, 0),
                    },
                },
                Event {
                    turn: 1,
                    start_time,
                    duration,
                    kind: EventKind::Die { piece_index: 6 },
                },
            ],
            ..BattleViewer::default()
        }
    }

    /// DELIVERABLE: a single `update()` landing both events' windows
    /// mid-flight drives both target pieces to correct, independent partial
    /// progress — neither stuck at its start state, neither jumped to its end
    /// state — and neither event leaks into the other piece.
    #[test]
    fn overlapping_move_and_die_on_different_pieces_both_progress_independently_mid_flight() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_overlapping_move_and_die(1.0, 1.0);

        let move_from = world_pos_for_cell(ACTIVE_COLS[0], TEAM_A_ROW);
        let move_to = world_pos_for_cell(5, 0);
        let die_start_scale = scene.pieces[6].transform.scale;
        let die_start_translate = scene.pieces[6].transform.translate;
        let move_start_scale = scene.pieces[0].transform.scale;

        scene.update(&mut ctx, Duration::from_secs_f32(1.5));

        // (a) Move piece (0): col/row committed instantly, translate mid-glide.
        assert_eq!(
            (scene.pieces[0].col, scene.pieces[0].row),
            (5, 0),
            "Move piece's col/row must already be committed to `to` mid-flight"
        );
        let move_x = scene.pieces[0].transform.translate.x;
        assert!(
            move_x > move_from.x.min(move_to.x) && move_x < move_from.x.max(move_to.x),
            "Move piece's translate.x ({move_x}) must be strictly between start ({}) and end ({}) \
             x while the Die event is simultaneously active",
            move_from.x,
            move_to.x
        );

        // (b) Die piece (6): still alive, scale shrinking but not yet zero.
        assert!(
            scene.pieces[6].alive,
            "Die piece must still be alive mid-flight, while the Move event is simultaneously \
             active"
        );
        let die_scale = scene.pieces[6].transform.scale;
        let die_mag = (die_scale.x * die_scale.x + die_scale.y * die_scale.y).sqrt();
        let start_mag = (die_start_scale.x * die_start_scale.x
            + die_start_scale.y * die_start_scale.y)
            .sqrt();
        assert!(
            die_mag > 0.0 && die_mag < start_mag,
            "Die piece's scale magnitude ({die_mag}) must be strictly between 0 and its starting \
             magnitude ({start_mag}) mid-flight"
        );

        // (c) Cross-independence: neither event leaks into the other piece.
        assert_eq!(
            scene.pieces[0].transform.scale, move_start_scale,
            "the Move piece's scale must be untouched by the simultaneously-active Die event"
        );
        assert_eq!(
            (scene.pieces[6].col, scene.pieces[6].row),
            (ACTIVE_COLS[2], TEAM_B_ROW),
            "the Die piece's col/row must be untouched by the simultaneously-active Move event"
        );
        assert_eq!(
            scene.pieces[6].transform.translate, die_start_translate,
            "the Die piece's translate must be untouched by the simultaneously-active Move event"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: BattleViewer scene wiring (b4-t4) — replaces fill_and_label with the
// real board + 4v4 (3 active + 1 bench per side, 8 pieces total) team-tinted
// idle-animating pieces.
// ─────────────────────────────────────────────────────────────────────────────

