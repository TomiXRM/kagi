use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;

use super::{
    collect_environment, fingerprint_repository, manifest_path, materialize_pristine,
    EnvironmentMeta, Fingerprint, FixtureManifest, HarnessError,
};

/// Request parsed by the thin `backend_probe` dispatcher.
#[derive(Clone, Debug)]
pub struct ProbeRequest {
    pub repo: PathBuf,
    pub operation: String,
    pub backend: String,
    pub candidate: Option<String>,
    pub git_executable: Option<PathBuf>,
    pub iterations: usize,
}

/// A downstream operation plugs into the common copy/timing/fingerprint runner.
/// Adding an operation changes only its own module plus the dispatcher's registry.
pub trait ProbeOperation: Sync {
    fn name(&self) -> &'static str;
    fn mutates_fixture(&self) -> bool;
    fn execute(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError>;
}

/// Immutable invocation context supplied to a registered operation.
pub struct ProbeContext<'a> {
    repo: &'a Path,
    backend: &'a str,
    candidate: Option<&'a str>,
    git_executable: Option<&'a Path>,
}

impl<'a> ProbeContext<'a> {
    pub fn repo(&self) -> &'a Path {
        self.repo
    }

    pub fn backend(&self) -> &'a str {
        self.backend
    }

    pub fn candidate(&self) -> Option<&'a str> {
        self.candidate
    }

    pub fn git_executable(&self) -> Option<&'a Path> {
        self.git_executable
    }
}

/// Process timing. CPU fields are `null` only where the OS cannot report them.
#[derive(Clone, Debug, Serialize)]
pub struct Timing {
    pub wall_ns: u128,
    pub user_ns: Option<u128>,
    pub sys_ns: Option<u128>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeIteration {
    pub iteration: usize,
    pub timing: Timing,
    pub before: Fingerprint,
    pub after: Fingerprint,
    pub canonical_output: Value,
}

/// Stable JSON envelope consumed by every P1–P6 report.
#[derive(Clone, Debug, Serialize)]
pub struct ProbeReport {
    pub schema_version: u32,
    pub operation: String,
    pub backend: String,
    pub candidate: Option<String>,
    pub git_executable: Option<String>,
    pub fixture_manifest: Option<FixtureManifest>,
    pub environment: EnvironmentMeta,
    pub mutates_fixture: bool,
    pub iterations: Vec<ProbeIteration>,
}

/// Run a registered operation without ever opening the template itself. Copy,
/// fingerprinting, and teardown are outside each operation's timed interval.
pub fn run_probe(
    request: &ProbeRequest,
    operations: &[&dyn ProbeOperation],
) -> Result<ProbeReport, HarnessError> {
    if request.iterations == 0 {
        return Err(HarnessError::new("--iterations must be positive"));
    }
    let operation = operations
        .iter()
        .copied()
        .find(|operation| operation.name() == request.operation)
        .ok_or_else(|| {
            HarnessError::new(format!(
                "operation '{}' is not registered",
                request.operation
            ))
        })?;
    let template = request
        .repo
        .canonicalize()
        .map_err(|error| HarnessError::io(&request.repo, error))?;
    let manifest = read_manifest(&template)?;
    let environment = collect_environment(Some(&template), manifest.clone())?;
    let scratch = tempfile::tempdir().map_err(|error| HarnessError::new(error.to_string()))?;
    let mut iterations = Vec::with_capacity(request.iterations);
    let mut expected_before: Option<Fingerprint> = None;

    if operation.mutates_fixture() {
        for iteration in 0..request.iterations {
            let copy = scratch.path().join(format!("run-{iteration}"));
            materialize_pristine(&template, &copy)?;
            iterations.push(run_one(
                operation,
                request,
                &copy,
                iteration,
                &mut expected_before,
            )?);
        }
    } else {
        let copy = scratch.path().join("warm-series");
        materialize_pristine(&template, &copy)?;
        for iteration in 0..request.iterations {
            iterations.push(run_one(
                operation,
                request,
                &copy,
                iteration,
                &mut expected_before,
            )?);
        }
    }

    Ok(ProbeReport {
        schema_version: 1,
        operation: request.operation.clone(),
        backend: request.backend.clone(),
        candidate: request.candidate.clone(),
        git_executable: request
            .git_executable
            .as_ref()
            .map(|path| path.to_string_lossy().replace('\\', "/")),
        fixture_manifest: manifest,
        environment,
        mutates_fixture: operation.mutates_fixture(),
        iterations,
    })
}

fn run_one(
    operation: &dyn ProbeOperation,
    request: &ProbeRequest,
    copy: &Path,
    iteration: usize,
    expected_before: &mut Option<Fingerprint>,
) -> Result<ProbeIteration, HarnessError> {
    let before = fingerprint_repository(copy)?;
    if let Some(expected) = expected_before {
        if expected != &before {
            return Err(HarnessError::new(format!(
                "pristine copy for iteration {iteration} differs from the first copy"
            )));
        }
    } else {
        *expected_before = Some(before.clone());
    }

    let context = ProbeContext {
        repo: copy,
        backend: &request.backend,
        candidate: request.candidate.as_deref(),
        git_executable: request.git_executable.as_deref(),
    };
    let cpu_before = process_cpu_time();
    let start = Instant::now();
    let canonical_output = operation.execute(&context)?;
    let wall_ns = start.elapsed().as_nanos();
    let cpu_after = process_cpu_time();
    let after = fingerprint_repository(copy)?;

    Ok(ProbeIteration {
        iteration,
        timing: Timing {
            wall_ns,
            user_ns: delta(cpu_before.map(|time| time.0), cpu_after.map(|time| time.0)),
            sys_ns: delta(cpu_before.map(|time| time.1), cpu_after.map(|time| time.1)),
        },
        before,
        after,
        canonical_output,
    })
}

fn read_manifest(template: &Path) -> Result<Option<FixtureManifest>, HarnessError> {
    let path = manifest_path(template);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| HarnessError::io(&path, error))?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn delta(before: Option<u128>, after: Option<u128>) -> Option<u128> {
    after
        .zip(before)
        .map(|(after, before)| after.saturating_sub(before))
}

#[cfg(unix)]
fn process_cpu_time() -> Option<(u128, u128)> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` points to writable storage with the exact C layout.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if status != 0 {
        return None;
    }
    // SAFETY: getrusage returned success and initialized `usage`.
    let usage = unsafe { usage.assume_init() };
    Some((timeval_ns(usage.ru_utime), timeval_ns(usage.ru_stime)))
}

#[cfg(unix)]
fn timeval_ns(time: libc::timeval) -> u128 {
    (time.tv_sec as u128).saturating_mul(1_000_000_000)
        + (time.tv_usec as u128).saturating_mul(1_000)
}

#[cfg(not(unix))]
fn process_cpu_time() -> Option<(u128, u128)> {
    None
}
