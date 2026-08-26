//! Diagnostic: print exactly what the commit pane and the WIP badge each see.
//!
//! The pane's file list comes from `working_tree_status`; the "+N −M" badge
//! comes from `staged_diffstat` + `unstaged_diffstat`. They are two different
//! Git queries, so they can disagree — this prints both side by side.
//!
//!     cargo run -p kagi-git --example status_dump -- /path/to/repo

use std::path::PathBuf;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let path = PathBuf::from(&arg);

    let backend = match kagi_git::Backend::open(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Backend::open({}) failed: {e}", path.display());
            std::process::exit(1);
        }
    };
    println!("repo workdir : {}", backend.path().display());
    match backend.head_state() {
        Ok(h) => println!("HEAD         : {h:?}"),
        Err(e) => println!("HEAD         : <error: {e}>"),
    }

    println!("\n── commit pane source: working_tree_status() ──");
    match backend.working_tree_status() {
        Ok(s) => {
            println!(
                "staged={}  unstaged={}  untracked={}  conflicted={}",
                s.staged.len(),
                s.unstaged.len(),
                s.untracked.len(),
                s.conflicted.len()
            );
            // An empty path means `entry_path` could not decode the name as
            // UTF-8; `build_file_tree` drops those rows, so the pane would
            // show fewer files than the counts above.
            let blank = s
                .staged
                .iter()
                .chain(s.unstaged.iter())
                .filter(|f| f.path.as_os_str().is_empty())
                .count()
                + s.untracked
                    .iter()
                    .chain(s.conflicted.iter())
                    .filter(|p| p.as_os_str().is_empty())
                    .count();
            if blank > 0 {
                println!("!! {blank} entr(ies) have an EMPTY path (undecodable filename)");
            }
            for f in s.staged.iter().take(5) {
                println!("  staged   {:?} {}", f.change, f.path.display());
            }
            for f in s.unstaged.iter().take(5) {
                println!("  unstaged {:?} {}", f.change, f.path.display());
            }
            for p in s.untracked.iter().take(5) {
                println!("  untracked {}", p.display());
            }
        }
        Err(e) => println!("!! working_tree_status FAILED: {e}"),
    }

    println!("\n── WIP badge source: staged/unstaged diffstat ──");
    let show = |label: &str, r: Result<Vec<kagi_git::FileDiffStat>, kagi_git::GitError>| match r {
        Ok(v) => {
            let (a, d) = v.iter().fold((0u32, 0u32), |(a, d), s| {
                (a + s.additions as u32, d + s.deletions as u32)
            });
            println!("{label}: {} file(s)  +{a} -{d}", v.len());
            for s in v.iter().take(5) {
                println!(
                    "    +{} -{}  {}",
                    s.additions,
                    s.deletions,
                    s.path.display()
                );
            }
        }
        Err(e) => println!("{label}: !! FAILED: {e}"),
    };
    show("staged  ", backend.staged_diffstat());
    show("unstaged", backend.unstaged_diffstat());

    println!(
        "\nNote: `unstaged_diffstat` covers TRACKED modifications only — it never\n\
         counts untracked files, so \"pane lists files / badge reads +0 -0\" is\n\
         expected for a purely untracked change. The reverse (badge non-zero,\n\
         pane empty) is the anomaly worth reporting."
    );
}
