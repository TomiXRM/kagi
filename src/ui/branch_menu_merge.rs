//! Cached, owner-bound delete-menu classification; no Git work during painting.
use super::{BranchKind, KagiApp};
use gpui::{AppContext, Context};

impl KagiApp {
    pub(crate) fn start_branch_menu_merge_check(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.branch_menu.as_mut() else {
            return;
        };
        if menu.kind != BranchKind::Local || menu.merge_check_started {
            return;
        }
        menu.merge_check_started = true;
        let name = menu.name.clone();
        let tip = menu.target.clone();
        let (Some(path), Some(head), Some(owner)) = (
            self.repo_path.clone(),
            self.view().head_oid.clone(),
            self.active_session()
                .and_then(|session| self.app_sessions.attachment(session)),
        ) else {
            return;
        };
        let read_key = self.reads.current_key(owner.session);
        let check_name = name.clone();
        let check_tip = tip.clone();
        let task = cx.background_spawn(async move {
            kagi_git::Backend::open(&path)
                .and_then(|repo| repo.branch_is_merged_into_head(&check_name, &check_tip, &head))
                .ok()
        });
        cx.spawn(async move |this, acx| {
            let merged = task.await;
            let _ = this.update(acx, |app, cx| {
                let current = app
                    .active_session()
                    .and_then(|session| app.app_sessions.attachment(session));
                if !app.reads.is_fresh(read_key)
                    || !current.is_some_and(|current| {
                        current.session == owner.session && current.visit == owner.visit
                    })
                {
                    return;
                }
                let Some(menu) = app.branch_menu.as_mut().filter(|menu| {
                    menu.kind == BranchKind::Local && menu.name == name && menu.target == tip
                }) else {
                    return;
                };
                menu.merged_into_current = merged;
                cx.notify();
            });
        })
        .detach();
    }
}
