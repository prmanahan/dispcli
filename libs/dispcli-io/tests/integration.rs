//! Integration tests for `dispcli-io` — Task 3 (native `ContentResolver`
//! and `DocumentSink` adapters, R3).
//!
//! Exercises the filesystem-backed implementations against real fixture
//! directories (via `tempfile`, never a shared path) — this crate is the
//! native-IO half of the R3 seam, so unlike `dispcli-core`'s in-memory
//! fakes, these tests intentionally touch disk.

use std::fs;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use dispcli_core::{ContentResolver, DocumentSink, ErrorKind};
use dispcli_io::{FsContentResolver, FsDocumentSink};

/// Guards `std::env::set_current_dir` — process-global state, same
/// discipline this workspace applies to `env::set_var` (TF1/TF2 in
/// `skills/rust.md`): serialize any test in this binary that mutates the
/// process CWD so they can't race each other.
static CWD_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// ============================================================================
// FsContentResolver
// ============================================================================

#[test]
fn resolver_reads_fixture_file_under_registry_dir() {
    let registry_dir = tempfile::tempdir().expect("tempdir should create");
    let skills_dir = registry_dir.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("fixture skills dir should create");
    fs::write(skills_dir.join("rust.md"), "rust skill content").expect("fixture file should write");

    let resolver = FsContentResolver::new(registry_dir.path());
    let content = resolver
        .resolve("rust", "skills/rust.md")
        .expect("fixture file should resolve");

    assert_eq!(content, "rust skill content");
}

#[test]
fn resolver_resolves_relative_to_registry_dir_not_process_cwd() {
    // The registry dir is an arbitrary tempdir, unrelated to the crate's
    // CWD under `cargo test`. A nested relative path only resolves if
    // the implementation joins against `registry_dir` — joining against
    // `std::env::current_dir()` instead would fail to find this file.
    let registry_dir = tempfile::tempdir().expect("tempdir should create");
    let nested_dir = registry_dir.path().join("nested").join("deeper");
    fs::create_dir_all(&nested_dir).expect("nested fixture dir should create");
    fs::write(nested_dir.join("block.md"), "nested block content")
        .expect("fixture file should write");

    let resolver = FsContentResolver::new(registry_dir.path());
    let content = resolver
        .resolve("block", "nested/deeper/block.md")
        .expect("nested path should resolve against the registry dir");

    assert_eq!(content, "nested block content");
}

#[test]
fn resolver_reports_resolution_failed_for_missing_file() {
    let registry_dir = tempfile::tempdir().expect("tempdir should create");

    let resolver = FsContentResolver::new(registry_dir.path());
    let err = resolver
        .resolve("rust", "skills/rust.md")
        .expect_err("missing file should fail to resolve");

    let expected_path = registry_dir.path().join("skills/rust.md");
    assert_eq!(err.kind, ErrorKind::ResolutionFailed);
    assert_eq!(err.detail("id"), Some("rust"));
    assert_eq!(
        err.detail("path"),
        Some(expected_path.display().to_string().as_str())
    );
    assert!(
        err.detail("cause").is_some(),
        "resolution_failed should carry the underlying io::Error cause"
    );
}

#[test]
fn resolver_rejects_absolute_declared_path() {
    // R3: resolution is relative to the registry file's directory.
    // Accepting an absolute path would silently break that contract
    // (`PathBuf::join` discards the base on an absolute RHS) — rejected
    // before any filesystem access is attempted.
    let registry_dir = tempfile::tempdir().expect("tempdir should create");

    let resolver = FsContentResolver::new(registry_dir.path());
    let err = resolver
        .resolve("escape", "/etc/passwd")
        .expect_err("absolute declared path should be rejected");

    assert_eq!(err.kind, ErrorKind::ResolutionFailed);
    assert_eq!(err.detail("id"), Some("escape"));
}

// ============================================================================
// FsDocumentSink
// ============================================================================

#[test]
fn sink_creates_missing_parent_dirs_and_writes_byte_exact_content() {
    let root = tempfile::tempdir().expect("tempdir should create");
    let out_path = root.path().join("nested").join("output").join("doc.md");
    let document = "line one\nline two\n";

    let sink = FsDocumentSink::new();
    sink.write(&out_path.display().to_string(), document)
        .expect("sink write should create parent dirs and succeed");

    let written = fs::read(&out_path).expect("written file should be readable");
    assert_eq!(written, document.as_bytes());
}

#[test]
fn sink_writes_when_parent_dir_already_exists() {
    let root = tempfile::tempdir().expect("tempdir should create");
    let out_path = root.path().join("doc.md");

    let sink = FsDocumentSink::new();
    sink.write(&out_path.display().to_string(), "already there")
        .expect("sink write into an existing dir should succeed");

    assert_eq!(
        fs::read_to_string(&out_path).expect("written file should be readable"),
        "already there"
    );
}

#[test]
fn sink_writes_bare_relative_filename_with_empty_parent() {
    // `Path::new("doc.md").parent()` is `Some("")` — empty, not `None` —
    // for a bare filename with no directory component. The
    // `!parent.as_os_str().is_empty()` guard in `FsDocumentSink::write`
    // exists to skip `create_dir_all("")` (which errors) for exactly this
    // shape, but neither sink test above exercises it: both build absolute
    // tempdir paths with a non-empty parent. Scope the relative write to a
    // tempdir CWD so it stays hermetic and never touches the repo.
    let _guard = CWD_LOCK.lock().expect("cwd lock should not be poisoned");
    let original_cwd = std::env::current_dir().expect("current dir should be readable");
    let root = tempfile::tempdir().expect("tempdir should create");
    std::env::set_current_dir(root.path()).expect("cwd should switch to tempdir");

    let sink = FsDocumentSink::new();
    let result = sink.write("doc.md", "bare filename content");

    std::env::set_current_dir(&original_cwd).expect("cwd should restore");

    result.expect("sink write with a bare relative filename should succeed");
    assert_eq!(
        fs::read_to_string(root.path().join("doc.md")).expect("written file should be readable"),
        "bare filename content"
    );
}

#[test]
fn sink_write_failure_maps_to_io_failed() {
    let root = tempfile::tempdir().expect("tempdir should create");
    // `blocker` is a regular file; treating it as a directory component
    // (`blocker/output.md`) makes `create_dir_all` fail on a real OS
    // error rather than panic.
    let blocker: &Path = &root.path().join("blocker");
    fs::write(blocker, "not a directory").expect("blocker fixture file should write");
    let out_path = blocker.join("output.md");

    let sink = FsDocumentSink::new();
    let err = sink
        .write(&out_path.display().to_string(), "content")
        .expect_err("writing under a non-directory path component should fail");

    assert_eq!(err.kind, ErrorKind::IoFailed);
}
