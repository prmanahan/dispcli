# Justfile for rust-template.
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
