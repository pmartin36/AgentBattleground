pub mod ability;
pub mod app;
pub mod asset_gen;
pub mod assets;
pub mod braille_name;
pub mod creatures;
pub mod diagnostics;
pub mod instructions;
#[cfg(test)]
mod lint_test_fixture;
pub mod mention;
pub mod player_data;
pub mod scene_id;
pub mod scenes;
pub mod sounds;
pub mod squad_role;
pub mod stamina;
pub mod stats;
pub mod registry;
pub mod cli;

pub use app::run;
