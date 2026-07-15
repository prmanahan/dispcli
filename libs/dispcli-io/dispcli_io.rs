//! dispcli-io — native IO adapters for `dispcli-core`.
//!
//! This crate is the native-only IO layer. It implements the R3 seam
//! traits (`ContentResolver`, `DocumentSink`) defined in `dispcli-core`
//! over the real filesystem — skill/profile/block content resolution and
//! assembled-document writes. The future mnemra WASM plugin replaces this
//! crate with host-function adapters implementing the same traits; the
//! core crate stays untouched either way.
//!
//! Task 3 scope: [`FsContentResolver`] and [`FsDocumentSink`], the native
//! `ContentResolver`/`DocumentSink` implementations. See
//! `docs/specs/0001-envelope-assembly.md` R3 and this crate's
//! `<path-resolution>` doc section below for the traversal decisions.

use std::fs;
use std::path::{Path, PathBuf};

use dispcli_core::{ContentResolver, DocumentSink, Error, ErrorKind};

// ============================================================================
// R3 — Native `ContentResolver`
// ============================================================================

/// Native filesystem `ContentResolver` (R3). Resolves a registry-declared
/// path relative to the **registry file's directory** — never the process
/// current working directory — and returns its content.
///
/// # Path-resolution / traversal decisions
///
/// This is the untrusted-path-resolution boundary of the crate; the
/// choices below are deliberate, not accidental:
///
/// - **Absolute declared paths are rejected** with a `resolution_failed`
///   error, before any filesystem access. `PathBuf::join` silently
///   *discards* the base path when the joined-in path is absolute, which
///   would otherwise let a registry entry resolve to anywhere the process
///   can read (e.g. `/etc/passwd`) despite R3/AC2.5 stating resolution is
///   relative to the registry directory. Rejecting closes this
///   absolute-path base-discard vector — it does not make the "relative
///   to the registry directory" contract fully enforced; see the next
///   decision below for the `..`-escape vector that is still open.
/// - **`..`-bearing relative paths are *not* blocked.** They can walk
///   above the registry directory. The registry (R2) is adopter-authored,
///   trusted input in v0 — the same trust tier as the profile/skill paths
///   it declares. Building subtree confinement (canonicalize-then-verify)
///   or an allowlist here would be over-engineering a control for a
///   surface that is not (yet) a trust boundary; deferred, not an
///   oversight.
/// - **Symlinks are followed** by `std::fs::read_to_string` with no
///   special handling — a symlink escaping the registry dir is reachable,
///   under the same trusted-registry rationale as `..`.
/// - **No panics on malformed input.** Non-UTF8, empty, or otherwise
///   unreadable paths surface as a `resolution_failed` `Error` carrying
///   the underlying `io::Error` message as `cause` — never `unwrap`/
///   `expect`. The trait takes `&str`, so non-UTF8 *path input* cannot
///   occur at this boundary; non-UTF8 *file content* is caught by
///   `read_to_string` returning `Err` (not a panic) and mapped the same
///   way.
#[derive(Debug)]
pub struct FsContentResolver {
    registry_dir: PathBuf,
}

impl FsContentResolver {
    /// Build a resolver rooted at `registry_dir` — the directory
    /// containing the registry TOML file (R3). The CLI (Task 5) derives
    /// this from the `--config` path's parent.
    #[must_use]
    pub fn new(registry_dir: impl Into<PathBuf>) -> Self {
        FsContentResolver {
            registry_dir: registry_dir.into(),
        }
    }
}

impl ContentResolver for FsContentResolver {
    fn resolve(&self, id: &str, path: &str) -> Result<String, Error> {
        if Path::new(path).is_absolute() {
            return Err(Error::resolution_failed(
                id,
                path,
                "declared path must be relative to the registry directory, got an absolute path",
            ));
        }

        let resolved = self.registry_dir.join(path);
        fs::read_to_string(&resolved).map_err(|cause| {
            Error::resolution_failed(id, resolved.display().to_string(), cause.to_string())
        })
    }
}

// ============================================================================
// R3 — Native `DocumentSink`
// ============================================================================

/// Native filesystem `DocumentSink` (R3). Writes the assembled document to
/// `path`, creating any missing parent directories first.
#[derive(Debug, Default, Clone, Copy)]
pub struct FsDocumentSink;

impl FsDocumentSink {
    #[must_use]
    pub fn new() -> Self {
        FsDocumentSink
    }
}

impl DocumentSink for FsDocumentSink {
    fn write(&self, path: &str, document: &str) -> Result<(), Error> {
        let target = Path::new(path);
        if let Some(parent) = target.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|cause| {
                Error::new(
                    ErrorKind::IoFailed,
                    format!("failed to create parent directories for '{path}': {cause}"),
                )
            })?;
        }

        fs::write(target, document).map_err(|cause| {
            Error::new(
                ErrorKind::IoFailed,
                format!("failed to write '{path}': {cause}"),
            )
        })
    }
}
