use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::modals::{BranchPlanKind, FieldTarget, PrField, PrFieldsModal};
use kagi::ui::KagiApp;
use std::time::{Duration, Instant};

use super::fixtures::{oid, Fixture};

macro_rules! modal {
    ($name:ident, $getter:ident, |$f:ident, $app:ident, $w:ident, $cx:ident| $body:block) => {
        pub(in crate::inventory) fn $name($f: &Fixture, $app: &Entity<KagiApp>, $w: AnyWindowHandle, $cx: &mut VisualTestAppContext) {
            let _ = (&$f, &$w);
            $body
            settle_plan($cx, $app);
            assert!($cx.read(|cx| $app.read(cx).$getter().is_some()), concat!(stringify!($name), " did not open its intended modal"));
        }
    };
}

pub(super) fn settle_plan(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).planning.is_some()) {
        cx.run_until_parked();
        assert!(Instant::now() < deadline, "modal plan did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
    cx.run_until_parked();
}

fn fill_input(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    input: fn(&KagiApp) -> gpui::Entity<gpui_component::input::InputState>,
    value: &str,
) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        let state = input(app.read(cx));
        state.update(cx, |state, cx| state.set_value(value, window, cx));
    })
    .unwrap();
    cx.advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
}

modal!(remote_browse, remote_browse, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_remote_browse(cx));
});
modal!(clone_card, clone_modal, |f, app, window, cx| {
    let listing = kagi_git::github_repos::RepoListing {
        name_with_owner: "acme/widgets".into(),
        host: "github.com".into(),
        is_fork: false,
        is_private: false,
        description: "Example fixture repository".into(),
        updated_at: String::new(),
    };
    app.update(cx, |app, cx| {
        app.open_clone_card(listing, cx);
        app.replan_clone(&f.repo, cx);
    });
});
modal!(update, update_modal, |f, app, window, cx| {
    use kagi_domain::update::{Asset, ReleaseInfo, UpdatePlan, Version};
    let latest = Version::parse("1.0.1").unwrap();
    let asset = Asset {
        name: "Kagi-test.dmg".into(),
        url: "https://example.invalid/Kagi-test.dmg".into(),
        size: 1,
    };
    app.update(cx, |app, cx| {
        app.update_available = Some((
            UpdatePlan {
                current: Version::parse("1.0.0").unwrap(),
                latest: latest.clone(),
                tag: "v1.0.1".into(),
                notes: "Release notes\nNew features".into(),
                asset: asset.clone(),
            },
            ReleaseInfo {
                tag: "v1.0.1".into(),
                version: latest,
                notes: "Release notes\nNew features".into(),
                assets: vec![asset],
            },
        ));
        app.open_update_modal();
        cx.notify();
    });
    kagi::ui::e2e::clear_control_bounds(window.window_id(), "active-modal/update");
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let bounds = kagi::ui::e2e::control_bounds(window.window_id(), "active-modal/update")
        .expect("Update card was rendered");
    assert!(
        bounds.size.width > gpui::px(0.) && bounds.size.height > gpui::px(0.),
        "Update card must have visible area: {bounds:?}"
    );
});
modal!(smart_commit, smart_commit_modal, |f, app, window, cx| {
    app.update(cx, |app, _| {
        app.set_smart_commit_modal(kagi::ui::smart_commit::SmartCommitModal::Consent)
    });
});
// AppNotice's presentation helper is also used by the real operation error path.
pub(super) fn app_notice(
    _: &Fixture,
    app: &Entity<KagiApp>,
    _: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, _| {
        kagi::ui::e2e::deliver_app_notice(app, "Unable to complete the operation")
    });
    assert!(cx.read(|cx| app.read(cx).app_notice().is_some()));
}
modal!(checkout, plan_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_plan_modal("feature", cx));
});
modal!(pull, pull_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_pull_modal(cx));
});
modal!(amend, amend_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| {
        app.open_amend_modal_with_message(
            kagi_git::AmendMode::MessageOnly,
            "Updated fixture commit".into(),
            cx,
        )
    });
});
modal!(pop, pop_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_pop_modal(0, cx));
});
modal!(stash_drop, stash_drop_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_stash_drop_modal(0, cx));
});
modal!(push_tag, push_tag_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_push_tag_modal("v1.0.0".into(), cx));
});
modal!(pr_merge, pr_merge_modal, |f, app, window, cx| {
    let pr = kagi_domain::github::PullRequest {
        number: 1013,
        head: "feature".into(),
        head_sha: oid(&f.repo, "HEAD~1").0,
        base: "main".into(),
        base_repo: "github.example/acme/base".into(),
        cross_repository: true,
        ..Default::default()
    };
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, true, cx)
    });
    settle_plan(cx, app);
});
modal!(pr_fields, pr_fields_modal, |f, app, window, cx| {
    app.update(cx, |app, _| {
        let owner = app.active_session().unwrap();
        app.set_pr_fields_modal(PrFieldsModal {
            generation: 1,
            target: FieldTarget::Pr {
                number: 1013,
                owner,
            },
            base_repo: "github.example/acme/base".into(),
            field: PrField::Labels,
            current: vec!["ui".into()],
            selected: vec!["ui".into()],
            candidates: Some(vec!["ui".into(), "design".into()]),
            error: None,
        });
    });
});
modal!(push, push_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_push_modal(cx));
});
modal!(branch_plan, branch_plan_modal, |f, app, window, cx| {
    app.update(cx, |app, _| {
        app.open_branch_plan_modal("main".into(), BranchPlanKind::PushSetUpstream)
    });
});
modal!(set_upstream, set_upstream_modal, |f, app, window, cx| {
    app.update(cx, |app, _| app.open_set_upstream_modal("main".into()));
});
modal!(rename_branch, rename_branch_modal, |f, app, window, cx| {
    app.update(cx, |app, _| app.open_rename_branch_modal("main".into()));
});
modal!(merge, merge_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), None, cx)
    });
    cx.run_until_parked();
});
modal!(
    tracking_checkout,
    tracking_checkout_modal,
    |f, app, window, cx| {
        app.update(cx, |app, _| {
            app.open_tracking_checkout_modal("origin/feature".into())
        });
    }
);
modal!(
    switch_to_latest,
    switch_to_latest_modal,
    |f, app, window, cx| {
        app.update(cx, |app, _| {
            app.open_switch_to_latest_modal("main".into(), "origin/main".into())
        });
    }
);
modal!(create_branch, create_branch_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| {
        app.open_create_branch_modal(oid(&f.repo, "HEAD"), cx)
    });
    fill_input(
        cx,
        app,
        window,
        |app| {
            app.create_branch_modal()
                .unwrap()
                .input_state
                .clone()
                .unwrap()
        },
        "inventory-feature",
    );
});
modal!(create_tag, create_tag_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| {
        app.open_create_tag_modal(oid(&f.repo, "HEAD"), cx)
    });
    fill_input(
        cx,
        app,
        window,
        |app| app.create_tag_modal().unwrap().input_state.clone().unwrap(),
        "v2.0.0",
    );
});
modal!(
    create_worktree,
    create_worktree_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| {
            app.open_create_worktree_modal(oid(&f.repo, "HEAD"), cx)
        });
        fill_input(
            cx,
            app,
            window,
            |app| {
                app.create_worktree_modal()
                    .unwrap()
                    .branch_state
                    .clone()
                    .unwrap()
            },
            "inventory-worktree",
        );
    }
);
modal!(
    unlock_worktree,
    unlock_worktree_modal,
    |f, app, window, cx| {
        app.update(cx, |app, _| app.open_unlock_worktree_modal("linked".into()));
    }
);
modal!(
    remove_worktree,
    remove_worktree_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| {
            app.open_remove_worktree_modal("linked".into(), true, cx)
        });
        cx.run_until_parked();
    }
);
modal!(
    worktree_lock_reason,
    worktree_lock_reason_modal,
    |f, app, window, cx| {
        app.update(cx, |app, _| app.open_lock_worktree_modal("linked".into()));
    }
);
modal!(lock_worktree, lock_worktree_modal, |f, app, window, cx| {
    app.update(cx, |app, _| app.open_lock_worktree_modal("linked".into()));
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let bounds = kagi::ui::e2e::control_bounds(window.window_id(), "worktree-lock-reason-review")
        .expect("review button");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
});
modal!(
    prune_worktrees,
    prune_worktrees_modal,
    |f, app, window, cx| {
        app.update(cx, |app, _| app.open_prune_worktrees_modal());
    }
);
modal!(
    repair_worktrees,
    repair_worktrees_modal,
    |f, app, window, cx| {
        app.update(cx, |app, _| app.open_repair_worktrees_modal());
    }
);
modal!(repo_health, repo_health_modal, |f, app, window, cx| {
    app.update(cx, |app, _| {
        app.open_repo_health_modal(kagi_domain::repo_health::HealthFix::WriteCommitGraph)
    });
});
modal!(
    apply_suggestion,
    apply_suggestion_modal,
    |f, app, window, cx| {
        let suggestion = kagi_domain::suggestion::Suggestion {
            path: "README.md".into(),
            start_line: 2,
            end_line: 2,
            replacement: "Suggested improvement".into(),
        };
        app.update(cx, |app, cx| {
            app.open_apply_suggestion_modal(suggestion, cx)
        });
    }
);
modal!(oplog_restore, oplog_restore_modal, |f, app, window, cx| {
    let entry = kagi_git::oplog::read_oplog_tail_for_repo(&f.repo, 20)
        .into_iter()
        .find(|entry| entry.op == "create-branch")
        .expect("seeded oplog entry");
    app.update(cx, |app, cx| {
        app.open_oplog_restore_modal(kagi_git::Operation::OpRevert { entry_id: entry.id }, cx)
    });
});

#[path = "modals_extra.rs"]
mod extra;
pub(super) use extra::*;
