//! The injectable seam between a completed `vid_gen` run's single video
//! output and the zero-padded PNG frame sequence
//! `operations::materialize_clip` reads. Production turns the video into
//! frames via the sibling `ffmpeg` binary; tests inject a fake that never
//! spawns a real subprocess.

use std::fmt;
use std::path::Path;

/// Turns the video sd-cli wrote for a `vid_gen` run into a zero-padded
/// `f_%03d.png` sequence in `frames_dir`. `video_out` is the exact `-o`
/// value the invocation was given; `frames`/`fps` describe the requested
/// clip.
pub trait FrameExtractor: Send + Sync + 'static {
    fn extract(
        &self,
        video_out: &Path,
        frames_dir: &Path,
        frames: u32,
        fps: u32,
    ) -> Result<(), FrameExtractError>;
}

/// Drives the sibling `ffmpeg` binary to turn a video into a PNG frame
/// sequence. The production default for every `AssetGen` constructor that
/// does not inject a test extractor.
pub struct FfmpegExtractor;

impl FrameExtractor for FfmpegExtractor {
    fn extract(
        &self,
        video_out: &Path,
        frames_dir: &Path,
        _frames: u32,
        fps: u32,
    ) -> Result<(), FrameExtractError> {
        // H3 writes its `-o` target as a still-image path, then emits the
        // actual video at that path with its extension swapped to `.avi`.
        let video = video_out.with_extension("avi");

        std::fs::create_dir_all(frames_dir).map_err(|e| FrameExtractError::Io(e.to_string()))?;

        let output = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&video)
            .arg("-vf")
            .arg(format!("fps={fps}"))
            .arg(frames_dir.join("f_%03d.png"))
            .output()
            .map_err(|e| FrameExtractError::Spawn(e.to_string()))?;

        if !output.status.success() {
            return Err(FrameExtractError::Process {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        Ok(())
    }
}

/// The one shared fake extractor every animation test injects instead of
/// spawning a real `ffmpeg`: reads the single image a fake `sd-cli` runner
/// wrote to `video_out` and replicates it `frames` times into `frames_dir`,
/// zero-padded in playback order.
#[cfg(test)]
pub(crate) struct DuplicatingFakeExtractor;

#[cfg(test)]
impl FrameExtractor for DuplicatingFakeExtractor {
    fn extract(
        &self,
        video_out: &Path,
        frames_dir: &Path,
        frames: u32,
        _fps: u32,
    ) -> Result<(), FrameExtractError> {
        let source = image::open(video_out).map_err(|e| FrameExtractError::Io(e.to_string()))?;
        std::fs::create_dir_all(frames_dir).map_err(|e| FrameExtractError::Io(e.to_string()))?;
        for i in 0..frames {
            source
                .save(frames_dir.join(format!("f_{i:03}.png")))
                .map_err(|e| FrameExtractError::Io(e.to_string()))?;
        }
        Ok(())
    }
}

/// A terminal extraction failure: launching `ffmpeg`, its exit status, or an
/// I/O failure reading the video or writing the frame files.
#[derive(Debug)]
pub enum FrameExtractError {
    Spawn(String),
    Process { code: Option<i32>, stderr: String },
    Io(String),
}

impl fmt::Display for FrameExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameExtractError::Spawn(msg) => write!(f, "failed to launch ffmpeg: {msg}"),
            FrameExtractError::Process { code, stderr } => {
                write!(f, "ffmpeg exited {code:?}: {stderr}")
            }
            FrameExtractError::Io(msg) => write!(f, "ffmpeg io error: {msg}"),
        }
    }
}
