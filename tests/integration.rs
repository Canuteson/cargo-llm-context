use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Each test gets its own output directory to avoid parallel-test races.
fn run_tool(fixture_name: &str, test_name: &str, extra_args: &[&str]) -> PathBuf {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-output")
        .join(test_name);
    std::fs::create_dir_all(&out).unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_cargo-llm-context"))
        .arg(fixture(fixture_name))
        .args(["--output-dir"])
        .arg(&out)
        .args(extra_args)
        .output()
        .expect("failed to run cargo-llm-context");

    assert!(
        result.status.success(),
        "tool failed:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    out
}

fn read(out: &Path, file: &str) -> String {
    std::fs::read_to_string(out.join(file))
        .unwrap_or_else(|_| panic!("output file {file} not found in {}", out.display()))
}

// ── ownership-fixture tests ───────────────────────────────────────────────────

#[test]
fn ownership_fixture_exits_successfully() {
    run_tool("ownership-fixture", "exits_successfully", &[]);
}

#[test]
fn owned_module_shows_owns_class() {
    let out = run_tool("ownership-fixture", "owned_module", &[]);
    let content = read(&out, "owned.md");
    assert!(content.contains("Buffer"), "expected Buffer type");
    assert!(content.contains("owns"), "expected 'owns' class for Buffer");
    assert!(content.contains("Registry"), "expected Registry type");
}

#[test]
fn shared_module_shows_shared_class() {
    let out = run_tool("ownership-fixture", "shared_module", &[]);
    let content = read(&out, "shared.md");
    assert!(content.contains("SharedCache"), "expected SharedCache type");
    assert!(
        content.contains("shared"),
        "expected 'shared' class for SharedCache"
    );
}

#[test]
fn handles_module_shows_handle_class() {
    let out = run_tool("ownership-fixture", "handles_module", &[]);
    let content = read(&out, "handles.md");
    assert!(content.contains("EntityId"), "expected EntityId");
    assert!(content.contains("handle"), "expected 'handle' class");
    assert!(content.contains("SlotIndex"), "expected SlotIndex");
}

#[test]
fn borrowed_module_shows_borrows_class() {
    let out = run_tool("ownership-fixture", "borrowed_module", &[]);
    let content = read(&out, "borrowed.md");
    assert!(content.contains("StrSlice"), "expected StrSlice");
    assert!(content.contains("borrows"), "expected 'borrows' class");
}

#[test]
fn internal_reexport_appears_in_lib_index() {
    let out = run_tool("ownership-fixture", "internal_reexport", &[]);
    let index = read(&out, "_index.md");
    // Forwarded is defined in a private module (`mod internal`) and re-exported via
    // `pub use internal::Forwarded`. The tool cannot mark it internal because private
    // modules never enter the known-module set — it surfaces but as "external".
    // The important invariant is that it appears at all.
    assert!(
        index.contains("Forwarded"),
        "expected Forwarded to appear via re-export"
    );
}

#[test]
fn ownership_module_doc_appears_in_output() {
    let out = run_tool("ownership-fixture", "module_doc", &[]);
    let content = read(&out, "owned.md");
    // The owned.rs module has an `## Ownership` doc comment.
    assert!(
        content.contains("sole owners"),
        "expected module ownership doc to be included"
    );
}

#[test]
fn merge_flag_produces_single_file() {
    let out = run_tool("ownership-fixture", "merge_flag", &["--merge"]);
    let merged = read(&out, "_merged.md");
    assert!(merged.contains("Buffer"));
    assert!(merged.contains("EntityId"));
    assert!(merged.contains("SharedCache"));
}
