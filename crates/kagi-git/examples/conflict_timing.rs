//! Where the time goes: the merge itself vs generating marker text.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let repo = git2::Repository::open(&a[1]).expect("open");
    let rev = |r: &str| {
        repo.revparse_single(r)
            .expect("revparse")
            .peel_to_commit()
            .expect("commit")
    };
    let (b, h) = (rev(&a[2]), rev(&a[3]));

    let t0 = std::time::Instant::now();
    let index = repo.merge_commits(&b, &h, None).expect("merge");
    let merge_ms = t0.elapsed().as_millis();

    let t1 = std::time::Instant::now();
    let n = index.conflicts().expect("conflicts").count();
    let list_ms = t1.elapsed().as_millis();

    println!("merge_commits      : {merge_ms} ms");
    println!("walk conflicts ({n}): {list_ms} ms");
    println!("=> the rest of the 5.8s is merge_file_from_index x {n}");
}
