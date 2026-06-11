mod git;

use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

use git::{Head, open_repository, snapshot};

/// State held by the GPUI application.
struct KagiApp {
    /// Primary display text shown in the window.
    message: SharedString,
    /// Sub-text lines (repo path, HEAD, etc.).
    details: Vec<SharedString>,
}

impl Render for KagiApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .child(
                div()
                    .text_xl()
                    .text_color(rgb(0xcdd6f4))
                    .child(self.message.clone()),
            );

        for detail in &self.details {
            col = col.child(
                div()
                    .text_color(rgb(0xa6adc8))
                    .child(detail.clone()),
            );
        }

        col
    }
}

fn main() {
    // Collect CLI arguments (skip argv[0]).
    let args: Vec<String> = std::env::args().skip(1).collect();

    let (message, details): (String, Vec<String>) = if args.is_empty() {
        (
            "Usage: kagi <repo-path>".to_string(),
            vec!["Provide the path to a git repository as the first argument.".to_string()],
        )
    } else {
        let repo_path = PathBuf::from(&args[0]);
        match open_repository(&repo_path) {
            Ok(info) => {
                let msg = format!("repo: {}", info.name);
                let mut details = vec![
                    format!("path: {}", info.workdir.display()),
                    format!("HEAD: {}", info.head.display()),
                ];

                // Log to stderr so automated tests can verify output without
                // needing to inspect the GPU window.
                eprintln!("[kagi] {}", msg);
                for d in &details {
                    eprintln!("[kagi] {}", d);
                }

                // ── Single Repository open + snapshot ───────────────────
                // T003/T004 used separate Repository::open calls (known debt).
                // T005 resolves this: open once, call snapshot() for all data.
                match git2::Repository::open(&repo_path) {
                    Ok(mut repo) => {
                        match snapshot(&mut repo, 10_000) {
                            Ok(snap) => {
                                // ── Working tree status ──────────────────
                                let status = &snap.status;
                                if status.is_dirty() {
                                    // Staged
                                    if !status.staged.is_empty() {
                                        let header =
                                            format!("Staged ({})", status.staged.len());
                                        eprintln!("[kagi] {}", header);
                                        details.push(header);
                                        for f in status.staged.iter().take(20) {
                                            let line = format!(
                                                "  [{}] {}",
                                                f.change.label(),
                                                f.path.display()
                                            );
                                            eprintln!("[kagi] {}", line);
                                            details.push(line);
                                        }
                                    }
                                    // Unstaged
                                    if !status.unstaged.is_empty() {
                                        let header =
                                            format!("Unstaged ({})", status.unstaged.len());
                                        eprintln!("[kagi] {}", header);
                                        details.push(header);
                                        for f in status.unstaged.iter().take(20) {
                                            let line = format!(
                                                "  [{}] {}",
                                                f.change.label(),
                                                f.path.display()
                                            );
                                            eprintln!("[kagi] {}", line);
                                            details.push(line);
                                        }
                                    }
                                    // Untracked
                                    if !status.untracked.is_empty() {
                                        let header =
                                            format!("Untracked ({})", status.untracked.len());
                                        eprintln!("[kagi] {}", header);
                                        details.push(header);
                                        for p in status.untracked.iter().take(20) {
                                            let line = format!("  {}", p.display());
                                            eprintln!("[kagi] {}", line);
                                            details.push(line);
                                        }
                                    }
                                    // Conflicted
                                    if !status.conflicted.is_empty() {
                                        let header =
                                            format!("Conflicted ({})", status.conflicted.len());
                                        eprintln!("[kagi] {}", header);
                                        details.push(header);
                                        for p in status.conflicted.iter().take(20) {
                                            let line = format!("  {}", p.display());
                                            eprintln!("[kagi] {}", line);
                                            details.push(line);
                                        }
                                    }
                                } else {
                                    let line = "Working tree clean".to_string();
                                    eprintln!("[kagi] {}", line);
                                    details.push(line);
                                }

                                // ── Commit log ──────────────────────────
                                let header = format!("Commits: {}", snap.commits.len());
                                eprintln!("[kagi] {}", header);
                                details.push(header);
                                for c in snap.commits.iter().take(3) {
                                    let line =
                                        format!("  {} {}", c.id.short(), c.summary);
                                    eprintln!("[kagi] {}", line);
                                    details.push(line);
                                }

                                // ── Branches ────────────────────────────
                                {
                                    let header = "BRANCHES:".to_string();
                                    eprintln!("[kagi] {}", header);
                                    details.push(header);

                                    let head_branch = match &snap.head {
                                        Head::Attached { branch, .. } => Some(branch.clone()),
                                        _ => None,
                                    };

                                    for b in &snap.branches {
                                        let mut line = b.name.clone();
                                        if let Some(up) = &b.upstream {
                                            if up.ahead > 0 {
                                                line.push_str(&format!(" \u{2191}{}", up.ahead));
                                            }
                                            if up.behind > 0 {
                                                line.push_str(&format!(" \u{2193}{}", up.behind));
                                            }
                                        }
                                        if head_branch.as_deref() == Some(b.name.as_str()) {
                                            line.push_str(" [HEAD]");
                                        }
                                        eprintln!("[kagi]   {}", line);
                                        details.push(format!("  {}", line));
                                    }
                                }

                                // ── Remote branches ─────────────────────
                                {
                                    let header = "REMOTE BRANCHES:".to_string();
                                    eprintln!("[kagi] {}", header);
                                    details.push(header);
                                    for rb in &snap.remote_branches {
                                        let line = format!("  {}/{}", rb.remote, rb.name);
                                        eprintln!("[kagi] {}", line);
                                        details.push(line);
                                    }
                                }

                                // ── Tags ────────────────────────────────
                                {
                                    let header = "TAGS:".to_string();
                                    eprintln!("[kagi] {}", header);
                                    details.push(header);
                                    for t in &snap.tags {
                                        let line = format!("  {}", t.name);
                                        eprintln!("[kagi] {}", line);
                                        details.push(line);
                                    }
                                }

                                // ── Stashes ─────────────────────────────
                                {
                                    let header = "STASHES:".to_string();
                                    eprintln!("[kagi] {}", header);
                                    details.push(header);
                                    for s in &snap.stashes {
                                        let line = format!(
                                            "  stash@{{{}}} {}",
                                            s.index, s.message
                                        );
                                        eprintln!("[kagi] {}", line);
                                        details.push(line);
                                    }
                                }
                            }
                            Err(e) => {
                                let line = format!("snapshot error: {}", e);
                                eprintln!("[kagi] {}", line);
                                details.push(line);
                            }
                        }
                    }
                    Err(e) => {
                        let line = format!("repo open error: {}", e.message());
                        eprintln!("[kagi] {}", line);
                        details.push(line);
                    }
                }

                (msg, details)
            }
            Err(e) => {
                let msg = format!("Error: {}", e);
                eprintln!("[kagi] {}", msg);
                (msg, vec![])
            }
        }
    };

    let message_shared = SharedString::from(message);
    let details_shared: Vec<SharedString> = details.into_iter().map(SharedString::from).collect();

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.), px(600.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| KagiApp {
                    message: message_shared.clone(),
                    details: details_shared.clone(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
