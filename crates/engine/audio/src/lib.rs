//! engine-audio — process-global, kira-backed audio subsystem.
mod backend;
mod cache;

pub use backend::{
    SoundHandle, init, is_muted, play, play_oneshot, set_bus_volume, set_master_volume, set_muted,
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

/// Options for [`play`]. `bus` selects the volume group. `loop_region`
/// (`RangeFrom<f64>` seconds) is opt-in looping: `None` plays once,
/// `Some(0.0..)` loops the whole track, `Some(3.5..)` plays an intro once then
/// loops the body from 3.5s to the end. `fade_in` and `volume` apply on start.
/// `Default` is a one-shot on the SFX bus at full volume — what [`play_oneshot`]
/// uses.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayOpts {
    pub bus: Bus,
    pub loop_region: Option<RangeFrom<f64>>,
    pub fade_in: Fade,
    pub volume: f32,
}

impl Default for PlayOpts {
    fn default() -> Self {
        Self { bus: Bus::Sfx, loop_region: None, fade_in: Fade::NONE, volume: 1.0 }
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
    fn play_opts_default_is_oneshot_sfx_full_volume() {
        let o = PlayOpts::default();
        assert_eq!(o.bus, Bus::Sfx);
        assert!(o.loop_region.is_none());
        assert_eq!(o.volume, 1.0);
    }

    #[test]
    fn play_opts_loop_region_start() {
        let opts = PlayOpts { loop_region: Some(3.5..), ..Default::default() };
        assert_eq!(opts.loop_region.unwrap().start, 3.5);
    }

    #[test]
    fn bus_variants_are_reachable() {
        let music = Bus::Music;
        let sfx = Bus::Sfx;
        assert_ne!(music, sfx);
    }
}
