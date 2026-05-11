use anyhow::Result;
use clap::Parser;

/// dispcli — dispatch envelope assembler.
///
/// v0 scaffold. The real CLI surface lands when the spec defines it
/// (see `docs/specs/`). Today this binary prints the core-crate version so
/// the build, lint, and CI surfaces are exercisable end-to-end.
#[derive(Parser, Debug)]
#[command(author, version, about = "Dispatch envelope assembler (v0 scaffold)")]
struct Args {}

fn main() -> Result<()> {
    let _ = Args::parse();
    println!("dispcli scaffold — core v{}", dispcli_core::version());
    Ok(())
}
