//! `kagi-mcp` binary — an MCP server over stdio JSON-RPC (#331).
//!
//! Usage:
//!   kagi-mcp stdio --repo <path> [--readonly]
//!
//! Read-only mode (#332): `--readonly` (or env `KAGI_MCP_READONLY=1|true`)
//! removes the write tool `kagi_confirm` from `tools/list`; calling it returns
//! method-not-found. The CLI flag wins over any other source of the setting.
//!
//! The repository is fixed at startup (PM-locked §5): tools take no `repo_path`,
//! so an agent connected to this server cannot reach any other repository.

use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

use kagi_mcp::{serve_stdio, Server};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // The only transport in v1 is `stdio`. Anything else prints usage.
    match args.first().map(String::as_str) {
        Some("stdio") => {}
        _ => {
            eprintln!("usage: kagi-mcp stdio --repo <path> [--readonly]");
            return ExitCode::from(2);
        }
    }

    let repo = match take_repo(&args[1..]) {
        Ok(r) => r,
        Err(msg) => {
            eprintln!("kagi-mcp: {}", msg);
            return ExitCode::from(2);
        }
    };

    if !repo.join(".git").exists() && !repo.join("HEAD").exists() {
        // Not fatal (git discovery may still find a parent repo), but warn so a
        // mistyped --repo is obvious in the host's logs.
        eprintln!(
            "kagi-mcp: warning: {} does not look like a git repo",
            repo.display()
        );
    }

    // CLI flag wins; the env var is the fallback source (#332).
    let readonly = args.iter().any(|a| a == "--readonly")
        || matches!(
            std::env::var("KAGI_MCP_READONLY").as_deref(),
            Ok("1") | Ok("true")
        );

    let mut server = Server::new(repo);
    server.set_readonly(readonly);
    let stdin = io::stdin();
    let stdout = io::stdout();
    if let Err(e) = serve_stdio(&mut server, BufReader::new(stdin.lock()), stdout.lock()) {
        eprintln!("kagi-mcp: I/O error: {}", e);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Pull `--repo <path>` out of the args (required).
fn take_repo(args: &[String]) -> Result<PathBuf, String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--repo" {
            return it
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--repo needs a path".to_string());
        }
    }
    Err("--repo <path> is required".to_string())
}
