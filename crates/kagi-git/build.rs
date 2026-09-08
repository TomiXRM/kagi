use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let lockfile = manifest_dir.join("../..").join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lockfile.display());
    let lockfile_contents = fs::read_to_string(&lockfile).unwrap();
    let version = lockfile_contents
        .split("\n\n")
        .find_map(|package| {
            package
                .lines()
                .any(|line| line == "name = \"git2\"")
                .then(|| {
                    package
                        .lines()
                        .find_map(|line| line.strip_prefix("version = \"")?.strip_suffix('"'))
                })
                .flatten()
        })
        .expect("Cargo.lock must record the resolved git2 crate version");
    println!("cargo:rustc-env=KAGI_GIT2_CRATE_VERSION={version}");
}
