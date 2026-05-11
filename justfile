# Justfile for dispcli.
# Run `just --list` to see available recipes.

# Default recipe — show help
default:
    @just --list

# Run cargo fmt --check, clippy, tests, and license/advisory check in order (CI gate).
# Requires cargo-deny: `cargo install --locked cargo-deny`
check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo deny check

# Auto-format all Rust source files.
fmt:
    cargo fmt

# Run the full test suite.
test:
    cargo test --workspace

# Generate coverage report and print a human-readable summary.
# Requires cargo-llvm-cov: `cargo install --locked cargo-llvm-cov`
coverage:
    cargo llvm-cov --workspace --lcov --output-path lcov.info
    cargo llvm-cov report --workspace --summary-only

# Build all workspace members in debug mode.
build:
    cargo build --workspace

# Build all workspace members in release mode.
release:
    cargo build --workspace --release

# Build the docs site.
# Requires: mdbook, mdbook-mermaid, mdbook-d2, and the d2 CLI on PATH.
# Install via: cargo install --locked mdbook mdbook-mermaid mdbook-d2
# d2 CLI: https://d2lang.com/tour/install
docs:
    mdbook build docs/

# Serve the docs site locally with live reload.
docs-serve:
    mdbook serve docs/
