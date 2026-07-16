//! dispcli — dispatch envelope assembler.
//!
//! Binary entrypoint. Parses args, wires `dispcli-io`'s native adapters
//! into `dispcli-core`'s IO-free assembly logic, and emits the R8 output
//! contract (a JSON summary on success, a JSON error on failure). Thin by
//! design — no assembly logic lives here (see `docs/specs/0001-envelope-assembly.md`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dispcli_core::{
    DocumentSink, Error, ErrorKind, SizeSummary, Summary, WorktreeSummary, assemble_standard,
    parse_registry, parse_request,
};
use dispcli_io::{FsContentResolver, FsDocumentSink};

#[derive(Parser, Debug)]
#[command(author, version, about = "Dispatch envelope assembler")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Assemble a dispatch envelope + prompt document from a request and registry (R9).
    Assemble(AssembleArgs),
}

#[derive(Parser, Debug)]
struct AssembleArgs {
    /// Dispatch request JSON path, or `-` to read from stdin. Required.
    #[arg(long)]
    request: String,
    /// Registry TOML path. Default: `$DISPCLI_CONFIG` if set, else
    /// `dispcli.toml` in the current directory.
    #[arg(long)]
    config: Option<String>,
    /// Document output path. Default:
    /// `{working_dir}/scratch/dispatch-{dispatch_id}-prompt.md`.
    #[arg(long)]
    out: Option<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match args.command {
        None => {
            println!("dispcli scaffold — core v{}", dispcli_core::version());
            ExitCode::SUCCESS
        }
        Some(Commands::Assemble(assemble_args)) => run_assemble(&assemble_args),
    }
}

/// Runs the `assemble` subcommand end-to-end and translates the R8 output
/// contract: a single JSON summary on stdout + exit 0 on success, or a
/// single `{"error": {...}}` JSON object on stderr + the error kind's
/// mapped exit code on failure (AC8.1/AC8.2).
fn run_assemble(args: &AssembleArgs) -> ExitCode {
    match try_assemble(args) {
        Ok(summary) => match serde_json::to_string(&summary) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(cause) => report_error(&Error::new(
                ErrorKind::IoFailed,
                format!("failed to serialize summary: {cause}"),
            )),
        },
        Err(err) => report_error(&err),
    }
}

/// Prints `{"error": {"kind", "message", "details"}}` to stderr (AC8.2)
/// and returns the process exit code the error's kind maps to.
fn report_error(err: &Error) -> ExitCode {
    let payload = serde_json::json!({ "error": err });
    match serde_json::to_string(&payload) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!(
            "{{\"error\":{{\"kind\":\"{}\",\"message\":\"internal: failed to serialize error payload\",\"details\":[]}}}}",
            err.kind
        ),
    }
    // exit_code() is a fixed 2..=7 range (R8 table) — try_from + a safe
    // fallback avoids a sign-losing `as` cast (clippy::cast_sign_loss).
    let code = u8::try_from(err.kind.exit_code()).unwrap_or(1);
    ExitCode::from(code)
}

/// Reads `--request` — the literal path, or stdin when the value is `-`
/// (R9). No file read happens in `dispcli-core`; that crate stays IO-free.
fn read_request_input(request_arg: &str) -> Result<String, Error> {
    if request_arg == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).map_err(|cause| {
            Error::new(
                ErrorKind::RequestInvalid,
                format!("failed to read request from stdin: {cause}"),
            )
        })?;
        Ok(buf)
    } else {
        std::fs::read_to_string(request_arg).map_err(|cause| {
            Error::new(
                ErrorKind::RequestInvalid,
                format!("failed to read request file '{request_arg}': {cause}"),
            )
        })
    }
}

/// Resolves `--config`'s effective path: flag, else `$DISPCLI_CONFIG`,
/// else `dispcli.toml` in the current directory (R9). Full precedence
/// documentation/tests (AC9.2) land in a later task; this is enough for
/// the happy fixture today.
fn resolve_config_path(flag: Option<&str>) -> PathBuf {
    if let Some(path) = flag {
        return PathBuf::from(path);
    }
    if let Ok(env_path) = std::env::var("DISPCLI_CONFIG") {
        return PathBuf::from(env_path);
    }
    PathBuf::from("dispcli.toml")
}

/// The `assemble` subcommand's fallible core: read + parse request and
/// registry, resolve content over the real filesystem, assemble the
/// document, write it, and build the R8 `Summary`.
fn try_assemble(args: &AssembleArgs) -> Result<Summary, Error> {
    let request_str = read_request_input(&args.request)?;
    let request = parse_request(&request_str)?;

    let config_path = resolve_config_path(args.config.as_deref());
    let registry_str = std::fs::read_to_string(&config_path).map_err(|cause| {
        Error::new(
            ErrorKind::ConfigInvalid,
            format!("failed to read config '{}': {cause}", config_path.display()),
        )
    })?;
    let registry = parse_registry(&registry_str)?;

    // Content resolution is rooted at the registry file's directory (R3),
    // never the process cwd.
    let registry_dir = config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let resolver = FsContentResolver::new(registry_dir);

    let assembled = assemble_standard(&request, &registry, &resolver)?;

    // assemble_standard already validated `request.agent` exists in the
    // registry (it would have returned request_invalid otherwise), so this
    // lookup succeeding is not a fresh assumption — the AssemblyFailed
    // fallback is defensive, not an expected path.
    let agent_entry = registry.agents.get(&request.agent).ok_or_else(|| {
        Error::new(
            ErrorKind::AssemblyFailed,
            format!(
                "agent '{}' not found in registry after successful assembly",
                request.agent
            ),
        )
    })?;

    let working_dir = request
        .envelope
        .worktree
        .clone()
        .unwrap_or_else(|| request.envelope.repo.clone());

    let out_path = args.out.clone().unwrap_or_else(|| {
        format!(
            "{working_dir}/scratch/dispatch-{}-prompt.md",
            request.envelope.dispatch_id
        )
    });

    let sink = FsDocumentSink::new();
    sink.write(&out_path, &assembled.document)?;

    let document_path = std::fs::canonicalize(&out_path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| out_path.clone());

    let total_bytes = assembled.components.iter().map(|c| c.bytes).sum();
    let mode = request.mode_override.unwrap_or(agent_entry.default_mode);

    Ok(Summary {
        document_path,
        agent: request.agent.clone(),
        tier: request.tier,
        weight: request.weight.clone(),
        mode,
        working_dir,
        worktree: WorktreeSummary {
            required: agent_entry.worktree_required,
            path: request.envelope.worktree.clone(),
            // Full argv-command generation is Task 10; v0 always reports
            // an empty command list.
            commands: Vec::new(),
        },
        size: SizeSummary {
            total_bytes,
            components: assembled.components,
        },
        verify_recipes: request.envelope.verify.clone(),
        // AC5.3 (spec 0001 Task 9) — unsupported brace-token warnings
        // `assemble_standard` collected across every skill/block section,
        // copied verbatim into the R8 summary for operator review.
        warnings: assembled.warnings,
    })
}
