/// Integration test for the `render_tier1` example (b3-t2).
///
/// Shells out to `cargo run -p game --example render_tier1 -- --once` and
/// verifies the headless `--once` mode prints colored braille to stdout.
/// This is the behavioral deliverable for b3-t2: the sprite rendered through
/// the real engine loop, captured headlessly.
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

/// `render_tier1 --once` must:
///   (a) exit 0
///   (b) print at least one braille glyph (U+2800..=U+28FF) to stdout
///   (c) print at least one ANSI truecolor escape (`\x1b[38;2;`) — proves color
///   (d) print at least 2 distinct braille codepoints — proves a real sprite,
///       not a blank or solid-fill stub (the disk's anti-aliased edge yields
///       partial glyphs alongside the full `⣿`)
#[test]
fn example_once_emits_colored_braille() {
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
            "render_tier1",
            "--",
            "--once",
        ])
        .current_dir(workspace_root)
        .output()
        .expect("failed to spawn `cargo run -p game --example render_tier1`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // (a) Must exit 0.
    assert!(
        output.status.success(),
        "render_tier1 --once must exit 0; got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout,
        stderr
    );

    // (b) Must contain at least one braille glyph.
    let has_braille = stdout
        .chars()
        .any(|c| ('\u{2800}'..='\u{28FF}').contains(&c));
    assert!(
        has_braille,
        "render_tier1 --once stdout must contain at least one braille glyph (U+2800..U+28FF);\
         \nstdout:\n{}",
        stdout
    );

    // (c) Must contain an ANSI truecolor escape — proves color was emitted.
    assert!(
        stdout.contains("\x1b[38;2;"),
        "render_tier1 --once stdout must contain an ANSI truecolor escape (\\x1b[38;2;);\
         \nstdout:\n{}",
        stdout
    );

    // (d) At least 2 distinct braille codepoints — real sprite, not a solid stub.
    let distinct_braille: HashSet<char> = stdout
        .chars()
        .filter(|c| ('\u{2800}'..='\u{28FF}').contains(c))
        .collect();
    assert!(
        distinct_braille.len() >= 2,
        "render_tier1 --once stdout must contain >= 2 distinct braille codepoints \
         (proves a real sprite, not a blank/solid stub); found {} distinct: {:?};\
         \nstdout:\n{}",
        distinct_braille.len(),
        distinct_braille,
        stdout
    );
}
