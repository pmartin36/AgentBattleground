//! Shared `#[cfg(test)]` fixtures for scene-core's own test suites
//! (`catalog.rs` + `scene::manager`) — ONE `MockScene`/`MockCatalog` pair,
//! reused rather than duplicated per-file (b3-t1).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::Frame;
use serde_json::Value as JsonValue;

use crate::inspect::FieldSchema;
use crate::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::scene_key::SceneKey;
use crate::{Inspectable, SceneCatalog};

/// Recorded interactions with a `MockScene`, shared via `Arc<Mutex<_>>` so a
/// test can inspect calls `SceneManager` makes on a scene it owns after
/// moving the scene into a `Box<dyn Scene>`.
#[derive(Default)]
pub struct MockSceneRecorder {
    pub entered_params: Vec<Option<JsonValue>>,
    pub exited: bool,
    pub last_key: Option<crossterm::event::KeyEvent>,
    pub last_mouse: Option<crossterm::event::MouseEvent>,
}

/// A `Scene` whose only editable state is `value: f32` — enough to exercise
/// `Inspectable::{schema,snapshot,apply_patch}` through the real derive
/// without any real game scene. `key` is fixed at construction (mirrors a
/// real scene's fixed `id()`).
#[derive(Inspectable)]
pub struct MockScene {
    #[inspect(hidden)]
    key: SceneKey,
    pub value: f32,
    #[inspect(hidden)]
    rec: Arc<Mutex<MockSceneRecorder>>,
    /// What this scene reports from `Scene::consumes_break` — stands in for a
    /// focused text field claiming Ctrl-C.
    #[inspect(hidden)]
    consumes_break: bool,
}

impl MockScene {
    pub fn new(key: SceneKey) -> Self {
        Self {
            key,
            value: 0.0,
            rec: Arc::new(Mutex::new(MockSceneRecorder::default())),
            consumes_break: false,
        }
    }

    /// Makes this scene claim the break key, as a scene with a focused text
    /// field does.
    pub fn consuming_break(key: SceneKey) -> Self {
        Self {
            consumes_break: true,
            ..Self::new(key)
        }
    }

    /// A handle to this scene's recorder, cloneable before the scene is
    /// moved into a `Box<dyn Scene>`.
    pub fn recorder(&self) -> Arc<Mutex<MockSceneRecorder>> {
        Arc::clone(&self.rec)
    }
}

impl Scene for MockScene {
    fn id(&self) -> SceneKey {
        self.key.clone()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, params: Option<JsonValue>) {
        self.rec.lock().unwrap().entered_params.push(params);
    }

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        match ev {
            InputEvent::Key(k) => self.rec.lock().unwrap().last_key = Some(k),
            InputEvent::Mouse(m) => self.rec.lock().unwrap().last_mouse = Some(m),
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {
        self.rec.lock().unwrap().exited = true;
    }

    fn inspect(&mut self) -> &mut dyn Inspectable {
        self
    }

    fn consumes_break(&self) -> bool {
        self.consumes_break
    }
}

/// Three-key catalog ("A", "B", "C") — `construct` builds a fresh
/// `MockScene` bound to the requested key; any other key is unavailable
/// (`is_available` == false) and panics if `construct`ed anyway, mirroring
/// the real `GameCatalog`'s panic-on-unbuilt-key contract.
pub struct MockCatalog;

impl SceneCatalog for MockCatalog {
    fn construct(&self, key: &SceneKey) -> Box<dyn Scene> {
        match key.as_str() {
            "A" | "B" | "C" => Box::new(MockScene::new(key.clone())),
            other => panic!("MockCatalog::construct: unbuilt key {other}"),
        }
    }

    fn schema_for(&self, key: &SceneKey) -> FieldSchema {
        match key.as_str() {
            "A" | "B" | "C" => <MockScene as Inspectable>::schema(),
            other => panic!("MockCatalog::schema_for: unbuilt key {other}"),
        }
    }

    fn display_name(&self, key: &SceneKey) -> &str {
        match key.as_str() {
            "A" => "Mock A",
            "B" => "Mock B",
            "C" => "Mock C",
            other => panic!("MockCatalog::display_name: unbuilt key {other}"),
        }
    }

    fn catalog_keys(&self) -> Vec<SceneKey> {
        vec![SceneKey::new("A"), SceneKey::new("B"), SceneKey::new("C")]
    }

    fn is_available(&self, key: &SceneKey) -> bool {
        matches!(key.as_str(), "A" | "B" | "C")
    }
}
