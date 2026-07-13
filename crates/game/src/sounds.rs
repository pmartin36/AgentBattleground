//! Bundled first-party sound bytes (spec `57-engine-audio-api.md`). The
//! audio parallel of `game::assets`: a flat module of `&'static [u8]`
//! `include_bytes!` consts. The bytes const IS the sound's identity — no
//! handle/ID/enum type. See `sounds/LICENSE.md` for provenance (CC0).

/// UI confirm click, `.ogg`. Played on cursor move / select in menu scenes.
pub const UI_CONFIRM: &[u8] = include_bytes!("sounds/ui_confirm.ogg");
