//! Create a deterministic, read-only #627 fixture template.
//!
//!     cargo run -p kagi-git --example backend_fixture -- \
//!       --out /tmp/kagi-s --files 200 --commits 50 --depth 3 --seed 627

use std::path::PathBuf;

use kagi_git::benchmark::{generate_fixture, FixtureRequest};

fn main() {
    let request = match parse_args() {
        Ok(request) => request,
        Err(error) => exit_error(&error),
    };
    match generate_fixture(&request) {
        Ok(manifest) => println!("{}", serde_json::to_string_pretty(&manifest).unwrap()),
        Err(error) => exit_error(&error.to_string()),
    }
}

fn parse_args() -> Result<FixtureRequest, String> {
    let mut out = None;
    let mut files = 200;
    let mut commits = 50;
    let mut depth = 3;
    let mut seed = 627;
    let mut scenario = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--out" => out = Some(PathBuf::from(value()?)),
            "--files" => files = parse_usize("--files", &value()?)?,
            "--commits" => commits = parse_usize("--commits", &value()?)?,
            "--depth" => depth = parse_usize("--depth", &value()?)?,
            "--seed" => {
                seed = value()?
                    .parse()
                    .map_err(|_| "--seed must be a u64".to_owned())?
            }
            "--scenario" => scenario = Some(value()?),
            "--help" | "-h" => return Err(usage().to_owned()),
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }
    Ok(FixtureRequest {
        output: out.ok_or_else(|| format!("--out is required\n{}", usage()))?,
        files,
        commits,
        depth,
        seed,
        scenario,
    })
}

fn parse_usize(flag: &str, value: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} must be a positive integer"))
}

fn usage() -> &'static str {
    "usage: backend_fixture --out PATH [--files N --commits N --depth N --seed N] [--scenario synthetic]"
}

fn exit_error(message: &str) -> ! {
    eprintln!("backend_fixture: {message}");
    std::process::exit(2)
}
