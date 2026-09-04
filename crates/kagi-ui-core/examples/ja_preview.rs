//! JA wording preview fixture (#376 follow-up).
//!
//! Renders a curated set of real plan scenarios (title + notes + recovery) so a
//! human can eyeball the *rendered* Japanese — not source diffs — and judge the
//! open wording questions (full-width parens, adjective `ローカル`, EN/JA
//! information parity).
//!
//! Run:
//!   cargo run -p kagi-ui-core --example ja_preview          # JA only
//!   cargo run -p kagi-ui-core --example ja_preview -- both  # JA + EN side by side
//!
//! Output is delimited plain text (`## case` / `--JA--` / `--EN--`) so it can be
//! diffed or piped into a formatter. Add a scenario to `cases()` to preview it.

use kagi_domain::plan_note::branch::{BranchNote, BranchRecovery, BranchTitle};
use kagi_domain::plan_note::checkout::CheckoutNote;
use kagi_domain::plan_note::merge::MergeNote;
use kagi_domain::plan_note::push::PushNote;
use kagi_domain::plan_note::reset::ResetNote;
use kagi_domain::plan_note::switch::SwitchNote;
use kagi_domain::plan_note::{PlanNote, PlanRecovery, PlanTitle, RecoveryKind};
use kagi_ui_core::i18n::{
    self,
    plan::{plan_note_text, plan_recovery_text, plan_title_text},
    Lang,
};

/// One preview scenario: what the UI would show for a single plan.
struct Case {
    /// Short human label + why it's interesting.
    label: &'static str,
    title: PlanTitle,
    notes: Vec<PlanNote>,
    recovery: Option<PlanRecovery>,
}

fn rec(kind: RecoveryKind) -> Option<PlanRecovery> {
    Some(PlanRecovery {
        kind,
        commands: vec![],
    })
}

fn s(v: &str) -> String {
    v.to_string()
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            label: "delete branch — squash-merged (ID分離 / backtick)",
            title: PlanTitle::Branch(BranchTitle::DeleteBranch {
                name: s("feature/login"),
                tip: Some(s("a1b2c3d")),
            }),
            notes: vec![PlanNote::Branch(BranchNote::DeleteSquashMerged {
                name: s("feature/login"),
                squash: s("main"),
            })],
            recovery: rec(RecoveryKind::Branch(BranchRecovery::DeleteBranch {
                name: s("feature/login"),
                tip: Some(s("a1b2c3d")),
            })),
        },
        Case {
            label: "delete branch — locked worktree (worktree ID行)",
            title: PlanTitle::Branch(BranchTitle::DeleteBranch {
                name: s("feature/wip"),
                tip: Some(s("deadbee")),
            }),
            notes: vec![PlanNote::Branch(BranchNote::DeleteBranchInLockedWorktree {
                name: s("feature/wip"),
                path: s("/repo/.wt/wip"),
            })],
            recovery: None,
        },
        Case {
            label: "create branch — 【判断①】EN/JA 情報差 (JAはHEAD不動の注記を削除)",
            title: PlanTitle::Branch(BranchTitle::CreateBranch {
                name: s("feature/new"),
                at: s("main"),
                checkout: true,
            }),
            notes: vec![],
            recovery: rec(RecoveryKind::Branch(BranchRecovery::CreateBranch {
                name: s("feature/new"),
            })),
        },
        Case {
            label: "switch — diverged (【判断②】全角括弧 （ahead/behind）)",
            title: PlanTitle::Branch(BranchTitle::RenameBranch {
                old: s("old"),
                new: s("new"),
            }),
            notes: vec![PlanNote::Switch(SwitchNote::DivergedSwitchOnly {
                name: s("feature/x"),
                remote: s("origin/feature/x"),
                ahead: 2,
                behind: 3,
            })],
            recovery: None,
        },
        Case {
            label: "reset — abandons commits + not-ancestor",
            title: PlanTitle::Reset(
                kagi_domain::plan_note::reset::ResetTitle::ResetCurrentToHead {
                    branch: s("main"),
                    to: s("a1b2c3d"),
                },
            ),
            notes: vec![
                PlanNote::Reset(ResetNote::AbandonsCommits {
                    branch: s("main"),
                    count: 4,
                }),
                PlanNote::Reset(ResetNote::TargetNotAncestor { branch: s("main") }),
            ],
            recovery: None,
        },
        Case {
            label: "push — no upstream / no remotes (command block)",
            title: PlanTitle::Push(kagi_domain::plan_note::push::PushTitle::Push {
                branch: s("main"),
                remote: s("origin"),
                set_upstream: true,
            }),
            notes: vec![PlanNote::Push(PushNote::NoUpstreamNoRemotes {
                branch: s("main"),
            })],
            recovery: None,
        },
        Case {
            label: "checkout — overlap (files行)",
            title: PlanTitle::Checkout(kagi_domain::plan_note::checkout::CheckoutTitle::Checkout {
                branch: s("dev"),
            }),
            notes: vec![PlanNote::Checkout(CheckoutNote::CheckoutOverlap {
                count: 3,
                files: s("a.rs, b.rs, c.rs"),
            })],
            recovery: None,
        },
        Case {
            label: "merge — target is current",
            title: PlanTitle::Merge(kagi_domain::plan_note::merge::MergeTitle::Into {
                target: s("feature/x"),
                current: Some(s("main")),
            }),
            notes: vec![PlanNote::Merge(MergeNote::TargetIsCurrent {
                target: s("feature/x"),
            })],
            recovery: None,
        },
    ]
}

fn render(c: &Case) {
    println!("{}", plan_title_text(&c.title));
    for n in &c.notes {
        println!("  • {}", plan_note_text(n).replace('\n', "\n    "));
    }
    if let Some(r) = &c.recovery {
        println!(
            "  ↩ {}",
            plan_recovery_text(Some(r)).replace('\n', "\n    ")
        );
    }
}

fn set_lang(l: Lang) {
    // init_lang() reads KAGI_LANG; flip it per pass without touching settings.json.
    std::env::set_var("KAGI_LANG", l.slug());
    i18n::init_lang();
}

fn main() {
    let both = std::env::args().any(|a| a == "both");
    let cases = cases();
    for c in &cases {
        println!("## {}", c.label);
        set_lang(Lang::Ja);
        println!("--JA--");
        render(c);
        if both {
            set_lang(Lang::En);
            println!("--EN--");
            render(c);
        }
        println!();
    }
}
