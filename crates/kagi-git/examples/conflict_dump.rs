//! Diagnostic: what `pr_conflict_preview` reports for two refs.
//!
//!     cargo run -p kagi-git --example conflict_dump -- <repo> <base-rev> <head-rev>

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (repo_path, base, head) = (a[1].clone(), a[2].clone(), a[3].clone());
    let repo = git2::Repository::open(&repo_path).expect("open");
    let rev = |r: &str| {
        kagi_git::CommitId(
            repo.revparse_single(r)
                .expect("revparse")
                .peel_to_commit()
                .expect("commit")
                .id()
                .to_string(),
        )
    };
    let (b, h) = (rev(&base), rev(&head));
    println!("base {} = {}", base, b.0);
    println!("head {} = {}", head, h.0);
    let t0 = std::time::Instant::now();
    match kagi_git::pr_conflict_files(&repo, &b, &h) {
        Ok(files) => {
            println!(
                "\n{} conflicted file(s) in {} ms (list only)",
                files.len(),
                t0.elapsed().as_millis()
            );
            for f in files.iter().take(5) {
                println!("  {:?}  {}", f.kind, f.path.display());
            }
            if let Some(first) = files.first() {
                let t1 = std::time::Instant::now();
                let text = kagi_git::pr_conflict_text(&repo, &b, &h, &first.path).unwrap();
                println!(
                    "\ntext for {}: {:?} bytes in {} ms",
                    first.path.display(),
                    text.as_ref().map(|t| t.len()),
                    t1.elapsed().as_millis()
                );
            }
        }
        Err(e) => println!("\n!! FAILED: {e}"),
    }
}
