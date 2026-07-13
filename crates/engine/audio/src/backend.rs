//! Kira-backed audio backend: `Backend` enum + graceful silent fallback +
//! mixer tracks + `init()` (b3-t1). Volume/mute state + setters (b3-t2).

use crate::{Bus, Fade, MusicOpts};
use kira::sound::Region;
use kira::sound::static_sound::StaticSoundHandle;
use kira::track::{TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend};
use kira::{Decibels, Tween};
use std::sync::{Mutex, OnceLock};

static AUDIO: OnceLock<Mutex<AudioState>> = OnceLock::new();

/// Device-independent top-level audio state. `mixer` round-trips on the
/// Silent (headless) backend; it must NOT live inside `Active`.
struct AudioState {
    backend: Backend,
    mixer: Mixer,
}

/// Pure, device-independent mixer state (b3-t2). Correctness (clamping,
/// mute round-trip) is unit-tested here with no global and no device.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mixer {
    master: f32,
    music: f32,
    sfx: f32,
    muted: bool,
}

impl Default for Mixer {
    // Trivial constant per blueprint (full volume, unmuted) -- not a
    // behavioral stub: `AudioState::new()` (b3-t1) must keep succeeding
    // without panicking, so this is implemented for real, unlike the
    // logic-bearing methods below.
    fn default() -> Self {
        Self { master: 1.0, music: 1.0, sfx: 1.0, muted: false }
    }
}

impl Mixer {
    /// Sets master amplitude, clamped to `0.0..=1.0`.
    fn set_master(&mut self, amp: f32) {
        self.master = amp.clamp(0.0, 1.0);
    }

    /// Sets the given bus's amplitude, clamped to `0.0..=1.0`.
    fn set_bus(&mut self, bus: Bus, amp: f32) {
        let amp = amp.clamp(0.0, 1.0);
        match bus {
            Bus::Music => self.music = amp,
            Bus::Sfx => self.sfx = amp,
        }
    }

    fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    fn is_muted(&self) -> bool {
        self.muted
    }

    /// `master` gated to silence when muted.
    fn effective_master(&self) -> f32 {
        if self.muted { 0.0 } else { self.master }
    }
}

/// Amplitude (`0.0..=1.0`) -> kira `Decibels`. `1.0` -> `Decibels::IDENTITY`;
/// `<=0.0` -> `Decibels::SILENCE`.
fn amplitude_to_db(amp: f32) -> Decibels {
    if amp <= 0.0 {
        Decibels::SILENCE
    } else if amp >= 1.0 {
        Decibels::IDENTITY
    } else {
        Decibels(20.0 * amp.log10())
    }
}

/// `Fade` -> a linear kira `Tween` of the given duration.
fn tween(fade: Fade) -> Tween {
    Tween { duration: fade.dur, ..Tween::default() }
}

/// Locks the global audio state, recovering from a poisoned mutex -- audio
/// never panics.
fn state() -> std::sync::MutexGuard<'static, AudioState> {
    audio().lock().unwrap_or_else(|e| e.into_inner())
}

/// Spec Decision 2's enum, verbatim. Device layer only.
#[allow(clippy::large_enum_variant)] // one process-lifetime instance behind a Mutex, never hot-path
enum Backend {
    /// kira is up.
    Active(Active),
    /// Device init failed; every op is a safe no-op.
    Silent,
}

struct Active {
    manager: AudioManager,
    music: TrackHandle,
    sfx: TrackHandle,
    /// The single currently-playing music track, if any. Owned here (not by
    /// the `SoundHandle` returned from `play_music`) so `stop_music`/the next
    /// `play_music` can fade/crossfade it -- `StaticSoundHandle` is
    /// single-owner, not `Clone`.
    current_music: Option<StaticSoundHandle>,
}

/// The lazy-init chokepoint every public API (b3-t2/t3) funnels through:
/// any first access brings the backend up if it isn't already.
fn audio() -> &'static Mutex<AudioState> {
    AUDIO.get_or_init(|| Mutex::new(AudioState::new()))
}

impl AudioState {
    fn new() -> Self {
        Self {
            backend: Backend::init_device(),
            mixer: Mixer::default(),
        }
    }

    /// Pushes `mixer`'s effective master level to kira. No-op on `Silent`.
    fn apply_master(&mut self, fade: Fade) {
        let db = amplitude_to_db(self.mixer.effective_master());
        if let Backend::Active(a) = &mut self.backend {
            a.manager.main_track().set_volume(db, tween(fade));
        }
    }

    /// Pushes `mixer`'s bus level to kira. No-op on `Silent`.
    fn apply_bus(&mut self, bus: Bus, fade: Fade) {
        let db = amplitude_to_db(match bus {
            Bus::Music => self.mixer.music,
            Bus::Sfx => self.mixer.sfx,
        });
        if let Backend::Active(a) = &mut self.backend {
            let handle = match bus {
                Bus::Music => &mut a.music,
                Bus::Sfx => &mut a.sfx,
            };
            handle.set_volume(db, tween(fade));
        }
    }
}

impl Backend {
    /// Attempts real device bring-up; on any error, logs one WARN and
    /// installs `Silent`. Never panics, never propagates.
    fn init_device() -> Backend {
        Backend::from_result(Self::try_active())
    }

    /// `AudioManager::new` + two sub-tracks (Music, Sfx), composed via `?`.
    fn try_active() -> Result<Active, Box<dyn std::error::Error>> {
        let mut manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())?;
        let music = manager.add_sub_track(TrackBuilder::new())?;
        let sfx = manager.add_sub_track(TrackBuilder::new())?;
        Ok(Active { manager, music, sfx, current_music: None })
    }

    /// `Ok` -> `Active`; `Err` -> one `tracing::warn!` + `Silent`.
    fn from_result<E: std::fmt::Display>(r: Result<Active, E>) -> Backend {
        match r {
            Ok(active) => Backend::Active(active),
            Err(e) => {
                tracing::warn!(error = %e, "audio device init failed; running silent");
                Backend::Silent
            }
        }
    }
}

/// Brings the audio backend up (or installs `Silent` on device failure).
/// Never panics. Idempotent -- repeat calls are safe no-ops after the first.
pub fn init() {
    let _ = audio();
}

/// Internal declick fade used by `set_muted` -- not user-facing, still
/// linear (matches kira's own default tween length).
const MUTE_FADE: Fade = Fade::ms(10);

/// Sets master (bus-of-busses) amplitude (b3-t2). No-op on `Silent`; never
/// panics. `amplitude` clamps to `0.0..=1.0`.
pub fn set_master_volume(amplitude: f32, fade: Fade) {
    let mut s = state();
    s.mixer.set_master(amplitude);
    s.apply_master(fade);
}

/// Sets the given bus's amplitude (b3-t2). No-op on `Silent`; never panics.
/// `amplitude` clamps to `0.0..=1.0`.
pub fn set_bus_volume(bus: Bus, amplitude: f32, fade: Fade) {
    let mut s = state();
    s.mixer.set_bus(bus, amplitude);
    s.apply_bus(bus, fade);
}

/// Mutes/unmutes master via an internal declick fade (b3-t2). No-op on
/// `Silent` beyond state tracking; never panics.
pub fn set_muted(muted: bool) {
    let mut s = state();
    s.mixer.set_muted(muted);
    s.apply_master(MUTE_FADE);
}

/// Current mute state (b3-t2). Round-trips even on `Silent`.
pub fn is_muted() -> bool {
    state().mixer.is_muted()
}

/// b3-t3 playback handle. `None` on `Silent` / a play error; every method is
/// a safe no-op in that case.
pub struct SoundHandle(Option<StaticSoundHandle>);

impl SoundHandle {
    /// Stops this sound with the given fade. No-op when `None`.
    pub fn stop(&mut self, fade: Fade) {
        if let Some(h) = &mut self.0 {
            h.stop(tween(fade));
        }
    }

    /// Sets this sound's volume with the given fade. No-op when `None`.
    pub fn set_volume(&mut self, amplitude: f32, fade: Fade) {
        if let Some(h) = &mut self.0 {
            h.set_volume(amplitude_to_db(amplitude), tween(fade));
        }
    }
}

/// Plays a one-shot on the SFX bus (concurrent -- kira auto-mixes). No-op
/// (returns a no-op handle) on `Silent`; never panics. Lazily initializes
/// the backend if called before `init()`.
pub fn play_sfx(sound: &'static [u8]) -> SoundHandle {
    let mut s = state();
    if let Backend::Active(a) = &mut s.backend {
        let data = crate::cache::sound_data(sound);
        return match a.sfx.play(data) {
            Ok(h) => SoundHandle(Some(h)),
            Err(_) => SoundHandle(None),
        };
    }
    SoundHandle(None)
}

/// Plays `sound` on the Music bus per `opts`, crossfading out any
/// currently-playing music. No-op on `Silent`; never panics. Lazily
/// initializes the backend if called before `init()`. The returned handle is
/// a placeholder (`None`) -- the backend retains the live handle internally
/// so `stop_music`/the next `play_music` can fade/crossfade it.
pub fn play_music(sound: &'static [u8], opts: MusicOpts) -> SoundHandle {
    let mut s = state();
    if let Backend::Active(a) = &mut s.backend {
        if let Some(mut old) = a.current_music.take() {
            old.stop(tween(opts.fade_in));
        }
        let data = crate::cache::sound_data(sound)
            .loop_region(music_loop_region(&opts))
            .volume(amplitude_to_db(opts.volume))
            .fade_in_tween(Some(tween(opts.fade_in)));
        if let Ok(h) = a.music.play(data) {
            a.current_music = Some(h);
        }
    }
    SoundHandle(None)
}

/// Fades the current music track to silence. No-op on `Silent` / when no
/// music is playing; never panics.
pub fn stop_music(fade: Fade) {
    let mut s = state();
    if let Backend::Active(a) = &mut s.backend {
        if let Some(mut h) = a.current_music.take() {
            h.stop(tween(fade));
        }
    }
}

/// Pure, device-free: `MusicOpts.loop_region` -> a kira `Region`. `None`
/// loops the whole track (start at 0s); `Some(start..)` starts the loop at
/// `start` seconds and runs to the end of the track.
fn music_loop_region(opts: &MusicOpts) -> Region {
    Region::from(opts.loop_region.clone().unwrap_or(0.0..))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::sound::PlaybackPosition;

    /// Host-independent proof of the graceful-fallback contract (spec lines
    /// 143-145): a device error must install `Silent`, never propagate or
    /// panic.
    #[test]
    fn fallback_installs_silent_on_device_error() {
        let backend = Backend::from_result::<String>(Err("boom".into()));
        assert!(matches!(backend, Backend::Silent));
    }

    /// `init()` must return normally on both Active (a machine with an audio
    /// device, e.g. local dev) and Silent (CI) -- never panic.
    #[test]
    fn init_does_not_panic() {
        init();
    }

    /// `init()` is idempotent: calling it twice must not panic or re-init.
    #[test]
    fn init_is_idempotent() {
        init();
        init();
    }

    /// Any API access before an explicit `init()` call must lazily bring the
    /// backend up without panicking.
    #[test]
    fn access_before_init_does_not_panic() {
        let _guard = audio().lock().expect("audio state mutex poisoned");
    }

    /// The deliverable's authoritative assertion (b3-t2): race-free because
    /// it exercises a local `Mixer`, never the process-global state shared
    /// across concurrently-running tests.
    #[test]
    fn mixer_mute_roundtrip() {
        let mut m = Mixer::default();
        m.set_master(0.7);
        m.set_muted(true);
        assert!(m.is_muted());
        assert_eq!(m.effective_master(), 0.0);
        m.set_muted(false);
        assert!(!m.is_muted());
        assert_eq!(m.master, 0.7);
        assert_eq!(m.effective_master(), 0.7);
    }

    #[test]
    fn mixer_clamps_amplitude() {
        let mut m = Mixer::default();
        m.set_master(1.5);
        assert_eq!(m.master, 1.0);
        m.set_master(-0.2);
        assert_eq!(m.master, 0.0);

        m.set_bus(Bus::Music, 1.5);
        assert_eq!(m.music, 1.0);
        m.set_bus(Bus::Music, -0.2);
        assert_eq!(m.music, 0.0);
    }

    #[test]
    fn amplitude_to_db_endpoints() {
        assert_eq!(amplitude_to_db(1.0), Decibels::IDENTITY);
        assert_eq!(amplitude_to_db(0.0), Decibels::SILENCE);
    }

    /// Global path (b3-t2): assert ONLY no-panic, never an exact `is_muted()`
    /// read back through the global -- concurrent tests share the
    /// process-global mute flag and would race otherwise.
    #[test]
    fn public_setters_do_not_panic_on_silent() {
        set_master_volume(0.5, Fade::NONE);
        set_bus_volume(Bus::Music, 0.3, Fade::ms(50));
        set_muted(true);
        set_muted(false);
        let _ = is_muted();
    }

    /// Encode a minimal 16-bit mono PCM WAV, leaked to `'static`. Decodable
    /// on `Active` (so this test is host-independent -- it must not panic
    /// whether the process ends up `Active` or `Silent`), and untouched by
    /// `Silent` entirely.
    fn test_wav_bytes() -> &'static [u8] {
        let samples: [i16; 4] = [1000, -1000, 500, 0];
        let sample_rate: u32 = 8000;
        let num_channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let byte_rate = sample_rate * num_channels as u32 * (bits_per_sample as u32 / 8);
        let block_align = num_channels * (bits_per_sample / 8);
        let data_size = (samples.len() * 2) as u32;

        let mut buf = Vec::with_capacity(44 + data_size as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_size).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        for s in samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        Box::leak(buf.into_boxed_slice())
    }

    /// Spec testing bullet (line 148), pure/device-free: `Some(3.5..)`
    /// yields a loop region starting at 3.5 seconds.
    #[test]
    fn music_loop_region_some_start_is_given_seconds() {
        let opts = MusicOpts { loop_region: Some(3.5..), ..Default::default() };
        let region = music_loop_region(&opts);
        assert_eq!(region.start, PlaybackPosition::Seconds(3.5));
    }

    /// `None` loops the whole track -- starts at 0s.
    #[test]
    fn music_loop_region_none_loops_whole_track_from_zero() {
        let opts = MusicOpts { loop_region: None, ..Default::default() };
        let region = music_loop_region(&opts);
        assert_eq!(region.start, PlaybackPosition::Seconds(0.0));
    }

    /// Spec lines 144-145: `play_sfx` and the handle it returns must never
    /// panic, on `Silent` or `Active`.
    #[test]
    fn play_sfx_and_handle_do_not_panic() {
        let mut handle = play_sfx(test_wav_bytes());
        handle.set_volume(0.5, Fade::ms(50));
        handle.stop(Fade::NONE);
    }

    /// Spec lines 144-145: `play_music` / `stop_music` must never panic, on
    /// `Silent` or `Active`, including the returned handle.
    #[test]
    fn play_music_and_stop_music_do_not_panic() {
        let mut handle = play_music(test_wav_bytes(), MusicOpts::default());
        handle.set_volume(0.8, Fade::NONE);
        stop_music(Fade::ms(100));
    }

    /// Lazy-init chokepoint: calling playback before an explicit `init()`
    /// must not panic (mirrors `access_before_init_does_not_panic` above).
    #[test]
    fn playback_before_explicit_init_does_not_panic() {
        let mut handle = play_sfx(test_wav_bytes());
        handle.stop(Fade::NONE);
    }
}
