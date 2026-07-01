/// Integration test for the `render_tier3` example (b3-t1).
///
/// Shells out to `cargo run -p game --example render_tier3 -- --once` and
/// verifies the headless `--once` mode prints two distinct colored-braille
/// frames to stdout, separated by `---FRAME-BREAK---`.
///
/// Assertions:
///   (a) exits 0
///   (b) stdout splits on the marker into exactly 2 non-empty parts
///   (c) each part contains at least 1 braille glyph (U+2800..=U+28FF)
///   (d) the two parts are NOT identical — proves the composited frame advanced
///   (e) stdout contains at least one ANSI truecolor escape `\x1b[38;2;` — color present
use std::path::PathBuf;
use std::process::Command;

const MARKER: &str = "---FRAME-BREAK---";

#[test]
fn example_once_emits_two_distinct_braille_frames() {
    // Workspace root is two directories above CARGO_MANIFEST_DIR (crates/game).
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .expect("crates/game must have a parent")
        .parent()
        .expect("crates must have a parent (workspace root)");

    let output = Command::new(env!("CARGO"))
        .args([
            "run",
            "-q",
            "-p",
            "game",
            "--example",
            "render_tier3",
            "--",
            "--once",
        ])
        .current_dir(workspace_root)
        .output()
        .expect("failed to spawn `cargo run -p game --example render_tier3`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // (a) Must exit 0.
    assert!(
        output.status.success(),
        "render_tier3 --once must exit 0; got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout,
        stderr
    );

    // (b) stdout must split on the marker into exactly 2 non-empty parts.
    let parts: Vec<&str> = stdout.splitn(2, MARKER).collect();
    assert_eq!(
        parts.len(),
        2,
        "render_tier3 --once stdout must contain exactly one '{}' marker; \
         got {} part(s);\nstdout:\n{}",
        MARKER,
        parts.len(),
        stdout
    );
    let frame_a = parts[0];
    let frame_b = parts[1];
    assert!(
        !frame_a.trim().is_empty(),
        "frame A (before marker) must not be empty;\nstdout:\n{}",
        stdout
    );
    assert!(
        !frame_b.trim().is_empty(),
        "frame B (after marker) must not be empty;\nstdout:\n{}",
        stdout
    );

    // (c) Each part must contain at least one braille glyph.
    let has_braille = |s: &str| s.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c));
    assert!(
        has_braille(frame_a),
        "frame A must contain at least one braille glyph (U+2800..U+28FF);\
         \nframe A:\n{}",
        frame_a
    );
    assert!(
        has_braille(frame_b),
        "frame B must contain at least one braille glyph (U+2800..U+28FF);\
         \nframe B:\n{}",
        frame_b
    );

    // (d) The two frames must differ — proves the composited animation actually advanced.
    assert!(
        frame_a != frame_b,
        "frame A and frame B must differ (composited animation must have advanced by one frame_dur);\
         \nframe A:\n{}\nframe B:\n{}",
        frame_a,
        frame_b
    );

    // (e) At least one ANSI truecolor escape — proves color was emitted.
    assert!(
        stdout.contains("\x1b[38;2;"),
        "render_tier3 --once stdout must contain an ANSI truecolor escape (\\x1b[38;2;);\
         \nstdout:\n{}",
        stdout
    );
}
