#[path = "backend_probe/d_f.rs"]
mod d_f;

use std::path::PathBuf;

use kagi_git::benchmark::{run_probe, ProbeRequest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut repo = None;
    let mut backend = None;
    let mut candidate = None;
    let mut iterations = None;
    let mut git_executable = None;

    while let Some(flag) = args.next() {
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

    let request = ProbeRequest {
        repo: repo.ok_or("--repo is required")?,
        operation: "execution-mixed".to_string(),
        backend: backend.ok_or("--backend is required")?,
        candidate,
        git_executable,
        iterations: iterations.ok_or("--iterations is required")?,
    };
    let operation = d_f::ExecutionMixed;
    let report = run_probe(&request, &[&operation])?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
