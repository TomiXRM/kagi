//! Run one registered #627 probe operation through the shared P0 envelope.
//!
//! This file stays a thin registry. Downstream packages implement
//! `ProbeOperation` in `backend_probe/{a,b,c,d_f,e}.rs`; adding their operation
//! changes that module and this registry, never the common P0 runner.

use std::path::PathBuf;

mod backend_probe {
    //! P2 owns `e`; the registry below is the only place it is wired in.
    pub mod b;
    pub mod e;
}

use kagi_git::benchmark::{run_probe, ProbeContext, ProbeOperation, ProbeRequest};
use serde_json::{json, Value};

struct Noop;

impl ProbeOperation for Noop {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn mutates_fixture(&self) -> bool {
        false
    }

    fn execute(
        &self,
        context: &ProbeContext<'_>,
    ) -> Result<Value, kagi_git::benchmark::HarnessError> {
        Ok(json!({
            "backend": context.backend(),
            "repo": context.repo().to_string_lossy(),
            "candidate": context.candidate(),
            "git_executable": context.git_executable().map(|path| path.to_string_lossy()),
        }))
    }
}

fn main() {
    let request = match parse_args() {
        Ok(request) => request,
        Err(error) => exit_error(&error),
    };
    let operations: [&dyn ProbeOperation; 3] = [
        &Noop,
        &backend_probe::b::SemanticMatrix,
        &backend_probe::e::CliCapability,
    ];
    match run_probe(&request, &operations) {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => exit_error(&error.to_string()),
    }
}

fn parse_args() -> Result<ProbeRequest, String> {
    let mut repo = None;
    let mut operation = None;
    let mut backend = None;
    let mut candidate = None;
    let mut git_executable = None;
    let mut iterations = 1;
    let mut format = "json".to_owned();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--repo" => repo = Some(PathBuf::from(value()?)),
            "--operation" => operation = Some(value()?),
            "--backend" => backend = Some(value()?),
            "--candidate" => candidate = Some(value()?),
            "--git-executable" => git_executable = Some(PathBuf::from(value()?)),
            "--iterations" => {
                iterations = value()?
                    .parse()
                    .map_err(|_| "--iterations must be a positive integer".to_owned())?
            }
            "--format" => format = value()?,
            "--help" | "-h" => return Err(usage().to_owned()),
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }
    if format != "json" {
        return Err("P0 supports only --format json".to_owned());
    }
    let backend = backend.ok_or_else(|| format!("--backend is required\n{}", usage()))?;
    if !matches!(backend.as_str(), "libgit2" | "cli" | "mixed") {
        return Err("--backend must be libgit2, cli, or mixed".to_owned());
    }
    Ok(ProbeRequest {
        repo: repo.ok_or_else(|| format!("--repo is required\n{}", usage()))?,
        operation: operation.ok_or_else(|| format!("--operation is required\n{}", usage()))?,
        backend,
        candidate,
        git_executable,
        iterations,
    })
}

fn usage() -> &'static str {
    "usage: backend_probe --repo PATH --operation NAME --backend libgit2|cli|mixed [--candidate ARG] [--git-executable PATH] [--iterations N] [--format json]"
}

fn exit_error(message: &str) -> ! {
    eprintln!("backend_probe: {message}");
    std::process::exit(2)
}
