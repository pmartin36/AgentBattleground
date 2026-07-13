//! engine-audio — process-global, kira-backed audio subsystem.
mod backend;
mod cache;

pub use backend::{
    SoundHandle, init, is_muted, play_music, play_sfx, set_bus_volume, set_master_volume,
    set_muted, stop_music,
};

use std::ops::RangeFrom;
use std::time::Duration;

/// A fade duration for a volume/track transition. `NONE` is an instant
/// (zero-duration) change; `ms` builds an explicit fade length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fade {
    pub dur: Duration,
}

impl Fade {
    pub const NONE: Fade = Fade { dur: Duration::ZERO };

    pub const fn ms(ms: u64) -> Fade {
        Fade { dur: Duration::from_millis(ms) }
    }
}

/// Which mixer bus a sound plays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    Music,
    Sfx,
}

/// Options for `play_music` (b3-t3). `loop_region` is `RangeFrom<f64>`
/// (seconds) — not `Range<f64>` — so `Some(3.5..)` means "loop the whole
/// track from 3.5s to the end"; `None` loops the entire track.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicOpts {
    pub loop_region: Option<RangeFrom<f64>>,
    pub fade_in: Fade,
    pub volume: f32,
}

impl Default for MusicOpts {
    fn default() -> Self {
        Self { loop_region: None, fade_in: Fade::NONE, volume: 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_none_is_zero() {
        assert_eq!(Fade::NONE.dur, Duration::ZERO);
    }

    #[test]
    fn fade_ms_sets_millis() {
        assert_eq!(Fade::ms(250).dur, Duration::from_millis(250));
    }

    #[test]
    fn music_opts_default_volume_full() {
        assert_eq!(MusicOpts::default().volume, 1.0);
    }

    #[test]
    fn music_opts_loop_region_start() {
        let opts = MusicOpts { loop_region: Some(3.5..), ..Default::default() };
        assert_eq!(opts.loop_region.unwrap().start, 3.5);
    }

    #[test]
    fn bus_variants_are_reachable() {
        let music = Bus::Music;
        let sfx = Bus::Sfx;
        assert_ne!(music, sfx);
    }
}
