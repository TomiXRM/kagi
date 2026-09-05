//! #454 visual check: launch the real app with one modal already open, so a
//! screenshot can be taken of the actual card.
//!
//! Why an example and not a scenario in `tests/gui_e2e_runner.rs`: the runner
//! drives `VisualTestAppContext`, whose platform wrapper renders windows
//! offscreen — `screencapture` cannot see them, and the pinned gpui has no
//! `render_to_image` for a Mac window (ADR-0166 §3). This binary uses the same
//! real platform as `main.rs`, so the window is a normal on-screen window.
//!
//! ```text
//! cargo run --example modal_shot -- <repo-path> amend|discard
//! ```
//!
//! Read-only: it builds the *plan* (`plan_amend` / status read) and opens the
//! confirmation modal. Nothing is executed — no button is pressed.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let repo = args.next().map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: modal_shot <repo-path> [amend|discard|checkout|push|stash|cherry-pick]");
        std::process::exit(2);
    });
    let which = args.next().unwrap_or_else(|| "amend".to_string());

    // Honour the settings theme like `main.rs` does: the panel tints are
    // theme-derived, so a capture must render the theme under test (#454 —
    // `surface == modal` on Apple Dark made the panels invisible).
    kagi::ui::theme::init_active();

    let mut app_state = kagi::ui::e2e::app_state(&repo).expect("open fixture repo");

    match which.as_str() {
        "amend" => {
            // `Both` (message + staged) is the mode whose plan carries
            // `preview_files` — the list that used to be cut at 10 rows.
            let backend = kagi_git::Backend::open(&repo).expect("open backend");
            let plan = backend
                .plan_amend(kagi_git::AmendMode::Both, Some("amended message"))
                .expect("plan_amend");
            app_state.set_amend_modal(kagi::ui::modals::AmendPlanModal {
                plan: std::sync::Arc::new(plan),
                error: None,
                mode: kagi_git::AmendMode::Both,
                message: "amended message".to_string(),
                confirm_armed: false,
            });
        }
        "discard" => {
            // Unstaged, tracked modifications are discard's targets; untracked
            // paths go in `skipped` (the collapsible section).
            let backend = kagi_git::Backend::open(&repo).expect("open backend");
            let snap = kagi_git::Backend::open(&repo)
                .expect("open backend")
                .snapshot(10_000)
                .expect("snapshot");
            let paths: Vec<String> = snap
                .status
                .unstaged
                .iter()
                .map(|f| f.path.display().to_string())
                .collect();
            let plan = backend.plan_discard(&paths).expect("plan_discard");
            // Untracked paths are what discard skips — the collapsible section.
            let skipped: Vec<String> = snap
                .status
                .untracked
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            app_state.set_discard_modal(kagi::ui::modals::DiscardModal {
                plan: std::sync::Arc::new(plan),
                paths,
                skipped,
                is_all: true,
                error: None,
                confirm_armed: false,
            });
        }
        // #454 layer 3: `push` is the plan that fills `preview_commits`, i.e.
        // the "Commits to push (N)" list that used to stop after 10 rows.
        "push" => {
            let backend = kagi_git::Backend::open(&repo).expect("open backend");
            let plan = backend.plan_push().expect("plan_push");
            app_state.set_push_modal(kagi::ui::modals::PushPlanModal {
                plan: std::sync::Arc::new(plan),
                error: None,
            });
        }
        // #454 layer 1: `checkout` goes through the SHARED plan card
        // (`render_plan_modal_card_styled`, ~14 modals), so this is the mode
        // that shows whether the shared card's footer stays fixed.
        "checkout" => {
            let backend = kagi_git::Backend::open(&repo).expect("open backend");
            let plan = backend.plan_checkout("feature/one").expect("plan_checkout");
            app_state.set_plan_modal(kagi::ui::modals::CheckoutPlanModal {
                target: kagi::ui::modals::CheckoutPlanTarget::Branch("feature/one".to_string()),
                stash_first: false,
                plan: std::sync::Arc::new(plan),
                error: None,
            });
        }
        // #454 layer 4: cards that adopted the shared shell in this slice.
        "stash" => {
            let mut backend = kagi_git::Backend::open(&repo).expect("open backend");
            let plan = backend
                .plan_stash_push(None, false)
                .expect("plan_stash_push");
            app_state.set_stash_push_modal(kagi::ui::modals::StashPushModal {
                input: String::new(),
                input_state: None,
                plan: Some(std::sync::Arc::new(plan)),
                error: None,
            });
        }
        "cherry-pick" => {
            let backend = kagi_git::Backend::open(&repo).expect("open backend");
            let head = backend.head_commit_id().expect("head commit");
            let plan = backend.plan_cherry_pick(&head).expect("plan_cherry_pick");
            app_state.set_cherry_pick_modal(kagi::ui::modals::CherryPickModal {
                commit_id: head,
                plan: std::sync::Arc::new(plan),
                error: None,
            });
        }
        other => {
            eprintln!(
                "second arg must be amend|discard|checkout|push|stash|cherry-pick, got {other:?}"
            );
            std::process::exit(2);
        }
    }

    kagi::ui::run_app(app_state);
}
