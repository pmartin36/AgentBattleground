//! Post-hatch "Add to Roster" action tests: button presence and placement,
//! for both an open roster slot and a full-roster pick-and-bump, with
//! persist round trips.

use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Condvar, Mutex};

use image::{Rgba, RgbaImage};

use crate::asset_gen::recipe::SdCliInvocation;
use crate::asset_gen::types::ImageAsset;
use crate::asset_gen::{CancelFlag, GpuCapability, JobError, JobRunner, RunOutput};

use super::*;

use super::hatch_sequence_tests as hsq;

/// Ticks `advance_hatch` until the sequence completes, or panics if the
/// timeline runs out first (a fixture bug, not a real timeout).
fn advance_to_complete(scene: &mut Hatchery) {
    for _ in 0..2000 {
        if scene.hatch.as_ref().unwrap().seq.is_complete() {
            return;
        }
        scene.advance_hatch(Duration::from_millis(5));
    }
    panic!("hatch sequence never completed");
}

/// A roster member distinguishable by name from the hatchling, for seeding
/// a pre-existing roster.
fn named_creature(name: &str) -> PersistedCreature {
    PersistedCreature::new(
        name,
        Element::Fire,
        Stats::default(),
        1,
        0,
        Vec::new(),
        Stamina::default(),
        None,
        None,
        None,
    )
}

/// Renders `scene`'s post-hatch roster-action UI directly, independent of
/// the scene's full render pass.
fn render_add_to_roster(scene: &Hatchery, w: u16, h: u16) -> ratatui::buffer::Buffer {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            scene.draw_add_to_roster(f, area);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

/// Before the hatch completes, no "Add to Roster" text has rendered.
#[test]
fn add_button_absent_before_completion() {
    let dir = temp_store_dir("hatch-roster-absent");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(named_creature("Newbie"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);

    let buf = render_add_to_roster(&scene, w, h);
    let text = crate::scenes::test_util::rect_text(&buf, Rect::new(0, 0, w, h));
    assert!(!text.contains("Add to Roster"), "expected no Add to Roster button before completion, got {text:?}");
}

/// Once the hatch completes and `maybe_offer_add_to_roster` runs, the
/// "Add to Roster" button renders.
#[test]
fn add_button_present_after_completion() {
    let dir = temp_store_dir("hatch-roster-present");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(named_creature("Newbie"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();

    let buf = render_add_to_roster(&scene, w, h);
    let text = crate::scenes::test_util::rect_text(&buf, Rect::new(0, 0, w, h));
    assert!(text.contains("Add to Roster"), "expected an Add to Roster button after completion, got {text:?}");
}

/// With an open roster slot, tapping the add button appends the hatchling
/// on disk, preserving the existing member's order.
#[test]
fn open_slot_add_grows_roster_persisted() {
    let dir = temp_store_dir("hatch-roster-open-slot");
    let seed = PlayerData {
        roster: vec![named_creature("Emberling")],
        eggs: vec![hsq::ready_egg_with_hatchling(named_creature("Newbie"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();

    let area = Rect::new(0, 0, w, h);
    let _ = render_add_to_roster(&scene, w, h);
    let focus_cell = focus::focus_layout(area).0.to_cell_rect();
    let rect = hatch_roster::add_button_rect(focus_cell);
    tap_at(&mut scene, rect.x + rect.width / 2, rect.y + rect.height / 2);

    let reloaded = PlayerStore::with_dir(&dir).load(|| panic!("must not fall back to seed")).into_data();
    assert_eq!(reloaded.roster.len(), 2, "roster must grow by one");
    assert_eq!(reloaded.roster[0].name, "Emberling", "existing member's order must be preserved");
    assert_eq!(reloaded.roster[1].name, "Newbie", "the hatchling must be appended");
}

/// With a full 6-slot roster, tapping the add button shows a pick UI
/// listing the current roster instead of placing the hatchling directly.
#[test]
fn full_roster_shows_picker() {
    let dir = temp_store_dir("hatch-roster-full-picker");
    let roster: Vec<PersistedCreature> =
        (0..crate::squad_role::ROSTER_SIZE).map(|i| named_creature(&format!("Member{i}"))).collect();
    let seed = PlayerData { roster, eggs: vec![hsq::ready_egg_with_hatchling(named_creature("Newbie"))] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();

    let area = Rect::new(0, 0, w, h);
    let _ = render_add_to_roster(&scene, w, h);
    let focus_cell = focus::focus_layout(area).0.to_cell_rect();
    let rect = hatch_roster::add_button_rect(focus_cell);
    tap_at(&mut scene, rect.x + rect.width / 2, rect.y + rect.height / 2);

    assert!(
        matches!(scene.roster_action, Some(hatch_roster::RosterAction::Picking { .. })),
        "a full roster must show the bump-picker instead of placing the hatchling directly"
    );
    let text = crate::scenes::test_util::rect_text(&render_add_to_roster(&scene, w, h), area);
    assert!(text.contains("Member2"), "expected a seeded roster name in the picker text, got {text:?}");
}

/// Picking a candidate in the full-roster picker replaces that slot with
/// the hatchling on disk; the roster stays size 6 and the picked creature
/// is gone.
#[test]
fn full_roster_bump_replaces_slot_persisted() {
    let dir = temp_store_dir("hatch-roster-full-bump");
    let roster: Vec<PersistedCreature> =
        (0..crate::squad_role::ROSTER_SIZE).map(|i| named_creature(&format!("Member{i}"))).collect();
    let seed = PlayerData { roster, eggs: vec![hsq::ready_egg_with_hatchling(named_creature("Newbie"))] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();

    let area = Rect::new(0, 0, w, h);
    let _ = render_add_to_roster(&scene, w, h);
    let focus_cell = focus::focus_layout(area).0.to_cell_rect();
    let add_rect = hatch_roster::add_button_rect(focus_cell);
    tap_at(&mut scene, add_rect.x + add_rect.width / 2, add_rect.y + add_rect.height / 2);

    let _ = render_add_to_roster(&scene, w, h);
    let panel = hatch_roster::picker_panel_rect(area, focus_cell);
    let pick_rect = hatch_roster::picker_button_rect(panel, 2);
    tap_at(&mut scene, pick_rect.x + pick_rect.width / 2, pick_rect.y + pick_rect.height / 2);

    let reloaded = PlayerStore::with_dir(&dir).load(|| panic!("must not fall back to seed")).into_data();
    assert_eq!(reloaded.roster.len(), crate::squad_role::ROSTER_SIZE, "roster must stay at full size");
    assert!(!reloaded.roster.iter().any(|c| c.name == "Member2"), "the picked creature must be gone");
    assert!(reloaded.roster.iter().any(|c| c.name == "Newbie"), "the hatchling must be present after the bump");
}

/// Once an open-slot add commits the hatchling, the hatch sub-mode ends:
/// the active hatch and its post-hatch action both clear, and the hatched
/// egg is retired from the tray, on the live scene and on disk.
#[test]
fn open_slot_add_dismisses_hatch_and_retires_egg() {
    let dir = temp_store_dir("hatch-roster-dismiss-open");
    let seed = PlayerData {
        roster: vec![named_creature("Emberling")],
        eggs: vec![hsq::ready_egg_with_hatchling(named_creature("Newbie"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();

    let area = Rect::new(0, 0, w, h);
    let _ = render_add_to_roster(&scene, w, h);
    let focus_cell = focus::focus_layout(area).0.to_cell_rect();
    let rect = hatch_roster::add_button_rect(focus_cell);
    tap_at(&mut scene, rect.x + rect.width / 2, rect.y + rect.height / 2);

    assert!(scene.hatch.is_none(), "hatch sub-mode must end once the hatchling is placed");
    assert!(scene.roster_action.is_none(), "the post-hatch action must clear on dismissal");
    assert!(scene.eggs.is_empty(), "the hatched egg must be retired from the tray");

    let reloaded = PlayerStore::with_dir(&dir).load(|| panic!("must not fall back to seed")).into_data();
    assert!(reloaded.eggs.is_empty(), "the retired egg must not survive on disk");
    assert_eq!(reloaded.roster.len(), 2, "the roster addition must still be persisted");
}

/// Once a full-roster bump commits the hatchling, the hatch sub-mode ends
/// the same way: the hatch clears and the hatched egg is retired, on the
/// live scene and on disk.
#[test]
fn full_roster_bump_dismisses_hatch() {
    let dir = temp_store_dir("hatch-roster-dismiss-full");
    let roster: Vec<PersistedCreature> =
        (0..crate::squad_role::ROSTER_SIZE).map(|i| named_creature(&format!("Member{i}"))).collect();
    let seed = PlayerData { roster, eggs: vec![hsq::ready_egg_with_hatchling(named_creature("Newbie"))] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();

    let area = Rect::new(0, 0, w, h);
    let _ = render_add_to_roster(&scene, w, h);
    let focus_cell = focus::focus_layout(area).0.to_cell_rect();
    let add_rect = hatch_roster::add_button_rect(focus_cell);
    tap_at(&mut scene, add_rect.x + add_rect.width / 2, add_rect.y + add_rect.height / 2);

    let _ = render_add_to_roster(&scene, w, h);
    let panel = hatch_roster::picker_panel_rect(area, focus_cell);
    let pick_rect = hatch_roster::picker_button_rect(panel, 2);
    tap_at(&mut scene, pick_rect.x + pick_rect.width / 2, pick_rect.y + pick_rect.height / 2);

    assert!(scene.hatch.is_none(), "hatch sub-mode must end once the bump completes");
    assert!(scene.eggs.is_empty(), "the hatched egg must be retired from the tray");

    let reloaded = PlayerStore::with_dir(&dir).load(|| panic!("must not fall back to seed")).into_data();
    assert!(reloaded.eggs.is_empty(), "the retired egg must not survive on disk");
}

/// After the hatch dismisses, the back button is reachable again: a
/// completed click on it returns a `Transition` to `RosterManager`, proving
/// input reaches the tray path instead of staying routed to the hatch.
#[test]
fn back_button_works_after_dismissal() {
    let dir = temp_store_dir("hatch-roster-dismiss-back");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(named_creature("Newbie"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();

    let area = Rect::new(0, 0, w, h);
    let _ = render_add_to_roster(&scene, w, h);
    let focus_cell = focus::focus_layout(area).0.to_cell_rect();
    let add_rect = hatch_roster::add_button_rect(focus_cell);
    tap_at(&mut scene, add_rect.x + add_rect.width / 2, add_rect.y + add_rect.height / 2);

    let _ = render_to_buffer(&scene, w, h);
    let back_rect = crate::scenes::home_button::home_dot_rect(area).to_cell_rect();
    let (cx, cy) = (back_rect.x, back_rect.y);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
    let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

    let t = t.expect("the back button must be reachable again after the hatch dismisses");
    assert_eq!(
        t.target,
        SceneKey::from(SceneId::RosterManager),
        "back button must return to RosterManager after dismissal"
    );
}

// --- multi-egg dismissal / clip_jobs index fixtures ---
//
// A `dismiss_hatch` removes the hatched egg from `eggs`/`art_cache`/
// `egg_buttons` in lockstep, shifting every surviving egg at a higher index
// down by one. `clip_jobs` (recorded idle/attack generation jobs, keyed by
// absolute egg index) must move in step with that shift, or a surviving
// egg's clip bookkeeping desyncs from its real tray position.

/// A text-gen factory that must never be invoked: these fixtures never
/// exercise the Done/definition pipeline.
fn unused_text_gen_factory() -> TextGenFactory {
    Box::new(|_cfg: &ResolvedModelConfig| -> TextGen {
        unreachable!("no text-gen pipeline exercised by clip desync tests")
    })
}

/// Writes a synthetic opaque still PNG to a unique temp path, standing in
/// for an already-resolved `egg_art`.
fn synthetic_still(tag: &str) -> ImageAsset {
    let path = std::env::temp_dir()
        .join(format!("game-hatchery-roster-clip-still-{}-{}.png", std::process::id(), tag));
    let mut img = RgbaImage::from_pixel(4, 4, Rgba([200, 60, 40, 255]));
    img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
    img.save(&path).unwrap();
    ImageAsset { path }
}

/// An `Incubating` egg with the given still and a fresh clipless hatchling
/// named `name`.
fn incubating_egg_with_art(egg_art: Option<ImageAsset>, name: &str) -> Egg {
    Egg {
        element: Element::Fire,
        state: EggState::Incubating { started_at: SystemTime::now() },
        mad_lib: Some("a small brave creature".to_string()),
        egg_art,
        hatchling: Some(named_creature(name)),
    }
}

/// Writes one synthetic frame to `invocation`'s output directory, standing
/// in for a successful animation-generation run.
fn write_synthetic_frame(invocation: &SdCliInvocation) {
    let o_idx = invocation.args.iter().position(|a| a == "-o").expect("-o arg present");
    let out_path = PathBuf::from(&invocation.args[o_idx + 1]);
    let dir = out_path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
    img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
    img.save(dir.join("f_000.png")).unwrap();
}

/// A `JobRunner` that counts invocations and always succeeds immediately,
/// with no artificial delay.
struct SucceedingRunner {
    calls: Arc<AtomicUsize>,
}

impl JobRunner for SucceedingRunner {
    fn run(&self, invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        write_synthetic_frame(invocation);
        Ok(RunOutput { stdout: String::new() })
    }
}

fn succeeding_asset_gen(calls: Arc<AtomicUsize>) -> AssetGen {
    AssetGen::new(Arc::new(SucceedingRunner { calls }), Box::new(ZImageBackend), GpuCapability::Available)
}

/// A `JobRunner` that succeeds on every call, but blocks calls numbered
/// `hold_from` and later until `release_gate` is called, letting a test hold
/// a job `Pending` across a scene mutation it wants to happen mid-flight.
struct GatedRunner {
    calls: Arc<AtomicUsize>,
    hold_from: usize,
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl JobRunner for GatedRunner {
    fn run(&self, invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if n >= self.hold_from {
            let (lock, cv) = &*self.gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = cv.wait(released).unwrap();
            }
        }
        write_synthetic_frame(invocation);
        Ok(RunOutput { stdout: String::new() })
    }
}

fn gated_asset_gen(calls: Arc<AtomicUsize>, hold_from: usize, gate: Arc<(Mutex<bool>, Condvar)>) -> AssetGen {
    AssetGen::new(Arc::new(GatedRunner { calls, hold_from, gate }), Box::new(ZImageBackend), GpuCapability::Available)
}

fn release_gate(gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, cv) = &**gate;
    *lock.lock().unwrap() = true;
    cv.notify_all();
}

/// After a sibling egg hatches and is dismissed, a still-incubating egg that
/// shifts into the dismissed egg's tray slot must still get its own
/// idle/attack clip generation submitted: a settled job the dismissed egg
/// left behind at that same index must not be mistaken for this egg's own
/// job.
#[test]
fn bystander_egg_gets_clips_after_sibling_dismissal() {
    let dir = temp_store_dir("clip-desync-suppress");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![
            incubating_egg_with_art(Some(synthetic_still("suppress-0")), "Hatched0"),
            incubating_egg_with_art(None, "Bystander1"),
        ],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let calls = Arc::new(AtomicUsize::new(0));
    let mut scene = Hatchery::from_store_with_gen(
        PlayerStore::with_dir(&dir),
        SystemTime::now(),
        succeeding_asset_gen(calls.clone()),
        None,
        unused_text_gen_factory(),
    );

    // Resolve egg 0's own idle+attack clips: exactly 2 jobs, recorded
    // against egg index 0. Egg 1 has no still yet, so it submits nothing.
    for _ in 0..200 {
        scene.advance_hatch_clips();
        if scene.eggs[0].hatchling.as_ref().is_some_and(|h| h.idle.is_some() && h.attack.is_some()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "fixture setup must resolve egg 0's own idle+attack clips before the hatch"
    );

    // Egg 0 is hatched and dismissed via the real "Add to Roster" path,
    // shifting egg 1 down to index 0.
    scene.eggs[0].state = EggState::Ready;
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();
    let area = Rect::new(0, 0, w, h);
    let _ = render_add_to_roster(&scene, w, h);
    let focus_cell = focus::focus_layout(area).0.to_cell_rect();
    let rect = hatch_roster::add_button_rect(focus_cell);
    tap_at(&mut scene, rect.x + rect.width / 2, rect.y + rect.height / 2);
    assert_eq!(scene.eggs.len(), 1, "fixture setup must leave the bystander as the sole remaining egg");
    assert_eq!(
        scene.eggs[0].hatchling.as_ref().map(|h| h.name.as_str()),
        Some("Bystander1"),
        "fixture setup must leave the bystander at index 0"
    );

    // The bystander's still now resolves, making it eligible for its own
    // clip job for the first time.
    scene.eggs[0].egg_art = Some(synthetic_still("suppress-bystander"));
    for _ in 0..200 {
        scene.advance_hatch_clips();
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        4,
        "the bystander egg that shifted into the dismissed egg's slot must get its own idle+attack \
         clip generation submitted, not suppressed by a stale job the dismissed egg left behind at \
         the same index"
    );
}

/// After a sibling egg hatches and is dismissed, an in-flight clip job
/// belonging to a surviving egg must resolve onto that same egg once it
/// shifts to a lower index, never onto whichever different egg now happens
/// to occupy the job's original (now stale) index.
#[test]
fn in_flight_clip_resolves_to_shifted_egg_not_stale_index() {
    let dir = temp_store_dir("clip-desync-misdirect");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![
            incubating_egg_with_art(Some(synthetic_still("misdirect-0")), "Hatched0"),
            incubating_egg_with_art(Some(synthetic_still("misdirect-1")), "Bystander1"),
            incubating_egg_with_art(None, "Bystander2"),
        ],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    // Egg 0's idle+attack (calls 1-2) resolve immediately; egg 1's idle+
    // attack (calls 3-4) hold in-flight until released. Egg 2 has no still,
    // so it never submits a job of its own.
    let mut scene = Hatchery::from_store_with_gen(
        PlayerStore::with_dir(&dir),
        SystemTime::now(),
        gated_asset_gen(calls.clone(), 3, gate.clone()),
        None,
        unused_text_gen_factory(),
    );

    for _ in 0..200 {
        scene.advance_hatch_clips();
        if scene.eggs[0].hatchling.as_ref().is_some_and(|h| h.idle.is_some() && h.attack.is_some()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        scene.eggs[0].hatchling.as_ref().map(|h| h.idle.is_some() && h.attack.is_some()),
        Some(true),
        "fixture setup must resolve egg 0's own clips while egg 1's job holds in-flight"
    );

    // Egg 0 is hatched and dismissed via the real "Add to Roster" path,
    // shifting egg 1 to index 0 and egg 2 to index 1 while its own job is
    // still in-flight.
    scene.eggs[0].state = EggState::Ready;
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_add_to_roster();
    let area = Rect::new(0, 0, w, h);
    let _ = render_add_to_roster(&scene, w, h);
    let focus_cell = focus::focus_layout(area).0.to_cell_rect();
    let rect = hatch_roster::add_button_rect(focus_cell);
    tap_at(&mut scene, rect.x + rect.width / 2, rect.y + rect.height / 2);
    assert_eq!(scene.eggs.len(), 2, "fixture setup must leave both bystanders after dismissal");
    assert_eq!(
        scene.eggs[1].hatchling.as_ref().map(|h| h.name.as_str()),
        Some("Bystander2"),
        "fixture setup must leave Bystander2 at index 1, the dismissed job's stale index"
    );

    // Let the held job resolve now that the shift has happened.
    release_gate(&gate);
    for _ in 0..200 {
        scene.advance_hatch_clips();
        std::thread::sleep(Duration::from_millis(2));
    }

    let wrongly_written = scene.eggs[1]
        .hatchling
        .as_ref()
        .map(|h| h.idle.is_some() || h.attack.is_some())
        .unwrap_or(false);
    assert!(
        !wrongly_written,
        "Bystander2 never had a clip job submitted for it and must not receive one resolved from \
         Bystander1's shifted-away index"
    );
}
