use super::*;
use kagi::ui::modals::{EditorFsPromptKind, EditorPendingIntent, TrustRepoModal};
use kagi_domain::branch_cleanup::{CleanupDeleteTarget, MergedBranchStatus};
use std::path::PathBuf;

modal!(stash_push, stash_push_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_stash_push_modal(cx));
    cx.run_until_parked();
});
modal!(stash_apply, stash_apply_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_stash_apply_modal(0, cx));
    cx.run_until_parked();
});
modal!(cherry_pick, cherry_pick_modal, |f, app, window, cx| {
    app.update(cx, |app, _| {
        app.open_cherry_pick_modal(oid(&f.repo, "feature"))
    });
});
modal!(revert, revert_modal, |f, app, window, cx| {
    app.update(cx, |app, _| app.open_revert_modal(oid(&f.repo, "HEAD")));
});
modal!(history, history_modal, |f, app, window, cx| {
    app.update(cx, |app, _| {
        app.ui_mut().expect("active session").operation_history = Default::default();
        app.record_history(
            kagi_git::OperationKind::Commit,
            "main",
            oid(&f.repo, "HEAD~1"),
            oid(&f.repo, "HEAD"),
            "second commit",
        );
        app.open_history_undo_modal();
    });
});
modal!(delete_branch, delete_branch_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| app.open_delete_branch_modal("merged", cx));
    settle_plan(cx, app);
});
modal!(
    delete_remote_branch,
    delete_remote_branch_modal,
    |f, app, window, cx| {
        app.update(cx, |app, _| {
            app.open_delete_remote_branch_modal("origin/feature")
        });
    }
);
modal!(reset_current, reset_current_modal, |f, app, window, cx| {
    app.update(cx, |app, cx| {
        app.open_reset_current_modal(oid(&f.repo, "HEAD~1"), cx)
    });
});
modal!(
    force_lease_push,
    force_lease_push_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| app.open_force_lease_push_modal(cx));
    }
);
modal!(
    rebase_current_onto,
    rebase_current_onto_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| app.open_rebase_modal("feature".into(), cx));
    }
);
modal!(
    branch_cleanup,
    branch_cleanup_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| {
            app.open_branch_cleanup_plan(
                vec![CleanupDeleteTarget {
                    name: "merged".into(),
                    local_tip: Some(oid(&f.repo, "HEAD~1")),
                    remote_tip: None,
                    status: MergedBranchStatus::FullyMerged,
                }],
                cx,
            )
        });
    }
);
modal!(discard, discard_modal, |f, app, window, cx| {
    let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
    app.update(cx, |app, cx| {
        app.open_discard_modal_for_path(
            owner,
            PathBuf::from("README.md"),
            kagi::ui::worktree_wip::WriteOrigin::CommitPanel,
            cx,
        )
    });
});

pub(in crate::inventory) fn conflict_continue(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.ui()
            .conflict
            .as_ref()
            .expect("sequencer conflict")
            .update(cx, |view, _| {
                view.mode
                    .as_mut()
                    .unwrap()
                    .buffer
                    .apply_choice(
                        std::path::Path::new("README.md"),
                        kagi_git::ResolutionChoice::Incoming,
                    )
                    .unwrap();
            });
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_continue(owner, window, cx);
        });
    })
    .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).conflict_continue_modal().is_some()),
        "resolved sequencer did not offer Continue"
    );
}
modal!(
    conflict_abort,
    conflict_abort_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| app.detect_conflict_mode(cx));
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                let owner = app
                    .active_session()
                    .and_then(|id| app.app_sessions.attachment(id))
                    .expect("conflict owner");
                app.open_conflict_abort_modal(owner, window, cx);
            });
        })
        .unwrap();
    }
);
modal!(
    editor_dirty_guard,
    editor_dirty_guard_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| {
            app.open_editor_dirty_guard(EditorPendingIntent::Close, cx)
        });
    }
);
modal!(
    editor_fs_prompt,
    editor_fs_prompt_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| {
            app.open_editor_fs_prompt(
                EditorFsPromptKind::Rename,
                PathBuf::from("README.md"),
                "renamed.md".into(),
                cx,
            )
        });
    }
);
modal!(
    editor_delete_confirm,
    editor_delete_confirm_modal,
    |f, app, window, cx| {
        app.update(cx, |app, cx| {
            app.open_editor_delete_confirm(PathBuf::from("README.md"), false, cx)
        });
    }
);
modal!(trust_repo, trust_repo_modal, |f, app, window, cx| {
    // A foreign UID cannot be manufactured by an unprivileged test process.
    app.update(cx, |app, _| {
        app.set_trust_repo_modal(TrustRepoModal {
            repo_path: f.repo.clone(),
            error: None,
        })
    });
});

modal!(
    trust_repo_save_error,
    trust_repo_modal,
    |f, app, window, cx| {
        let store =
            PathBuf::from(std::env::var("KAGI_LOG_DIR").unwrap()).join("trusted_repos.json");
        let original = std::fs::read(&store).ok();
        if store.is_file() {
            std::fs::remove_file(&store).unwrap();
        }
        std::fs::create_dir(&store).unwrap();
        app.update(cx, |app, cx| {
            app.set_trust_repo_modal(TrustRepoModal {
                repo_path: f.repo.clone(),
                error: None,
            });
            // Use the production save-stage formatter and the real directory-store
            // I/O error rather than reproducing either string in the inventory.
            app.confirm_trust_repo(cx);
        });
        std::fs::remove_dir(&store).unwrap();
        if let Some(original) = original {
            std::fs::write(&store, original).unwrap();
        }
        assert!(cx.read(|cx| app
            .read(cx)
            .trust_repo_modal()
            .and_then(|modal| modal.error.as_ref())
            .is_some()));
    }
);
