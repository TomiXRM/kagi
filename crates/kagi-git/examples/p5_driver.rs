#[path = "backend_probe/d_f.rs"]
mod d_f;

use std::path::PathBuf;
use std::time::Instant;

use serde_json::json;

use kagi_git::benchmark::{run_probe, ProbeRequest};
use kagi_git::{run_git, Backend};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut repo = None;
    let mut backend = None;
    let mut candidate = None;
    let mut iterations = None;
    let mut warm_index = None;
    let mut git_executable = None;

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--warm-index" => warm_index = Some(true),
            "--warm-index-ready" => warm_index = Some(false),
            "--repo" | "--backend" | "--candidate" | "--iterations" | "--git-executable" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                match flag.as_str() {
                    "--repo" => repo = Some(PathBuf::from(value)),
                    "--backend" => backend = Some(value),
                    "--candidate" => candidate = Some(value),
                    "--iterations" => iterations = Some(value.parse()?),
                    "--git-executable" => git_executable = Some(PathBuf::from(value)),
                    _ => return Err(format!("unknown flag {flag}").into()),
                }
            }
            _ => return Err(format!("unknown flag {flag}").into()),
        }
    }

    let request = ProbeRequest {
        repo: repo.ok_or("--repo is required")?,
        operation: "execution-mixed".to_string(),
        backend: backend.ok_or("--backend is required")?,
        candidate,
        git_executable,
        iterations: iterations.ok_or("--iterations is required")?,
    };
    if let Some(prime_index) = warm_index {
        return run_warm_index_series(&request, prime_index);
    }

    let operation = d_f::ExecutionMixed;
    let report = run_probe(&request, &[&operation])?;
    let timings = report
        .iterations
        .iter()
        .map(|iteration| {
            json!({
                "iteration": iteration.iteration,
                "wall_ns": iteration.timing.wall_ns,
                "user_ns": iteration.timing.user_ns,
                "sys_ns": iteration.timing.sys_ns,
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "backend": report.backend,
            "candidate": report.candidate,
            "environment": report.environment,
            "fixture_manifest": report.fixture_manifest,
            "iterations": timings,
        }))?
    );
    Ok(())
}

fn run_warm_index_series(
    request: &ProbeRequest,
    prime_index: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if request.candidate.as_deref() != Some("d1-working-tree-status") {
        return Err("candidate must be d1-working-tree-status".into());
    }
    if prime_index {
        let prime = run_git(
            &request.repo,
            &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        )
        .map_err(|error| error.to_string())?;
        if prime.status != 0 {
            return Err(format!(
                "index prime exited {}: {}",
                prime.status,
                prime.stderr.trim()
            )
            .into());
        }
    }

    let mut timings = Vec::with_capacity(request.iterations);
    for round in 0..request.iterations + 2 {
        let start = Instant::now();
        match request.backend.as_str() {
            "libgit2" => {
                let backend = Backend::open(&request.repo).map_err(|error| error.to_string())?;
                backend
                    .working_tree_status()
                    .map_err(|error| error.to_string())?;
            }
            "cli" => {
                let output = run_git(
                    &request.repo,
                    &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
                )
                .map_err(|error| error.to_string())?;
                if output.status != 0 {
                    return Err(format!(
                        "git status exited {}: {}",
                        output.status,
                        output.stderr.trim()
                    )
                    .into());
                }
            }
            other => return Err(format!("backend must be libgit2 or cli, got {other}").into()),
        }
        if round >= 2 {
            timings.push(start.elapsed().as_nanos());
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "backend": request.backend,
            "candidate": request.candidate,
            "index_primed": prime_index,
            "index_primed_with": "run_git status --porcelain=v2 -z",
            "warmups": 2,
            "timings_ns": timings,
        }))?
    );
    Ok(())
}
