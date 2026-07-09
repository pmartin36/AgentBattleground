//! Inspector UI state and egui app (b5-t2).
//!
//! `SwitcherState` is the pure, egui-free controller layer. All UI-relevant
//! state lives here and is updated through `apply(&Message)`. The egui draw
//! layer (`InspectorApp`) is built on top of this struct; tests only target
//! `SwitcherState`.

mod fields;
mod inspector_app;
mod state;

pub use inspector_app::InspectorApp;

#[cfg(test)]
mod test_fixtures;
