//! Native folder dialogs fail independently of any Git operation.
use super::{i18n, KagiApp, ToastKind};
use gpui::Context;
use kagi_git::{
    oplog::{OpLogEntry, OpOutcome},
    ops::StateSummary,
};
use std::path::PathBuf;

impl KagiApp {
    /// ToastStack bounds the preview; active-repo context retains durable detail.
    /// Home has no repository receipt: diagnostics stay in the process log.
    pub(crate) fn report_path_picker_failure(
        &mut self,
        owner: Option<PathBuf>,
        op: &str,
        label: &str,
        error: String,
        cx: &mut Context<Self>,
    ) {
        let display = i18n::Msg::PathPickerFailed
            .t()
            .replace("{picker}", label)
            .replace("{error}", &error);
        if let Some(repo) = owner {
            let entry = OpLogEntry::new(
                op,
                repo.display().to_string(),
                StateSummary {
                    head: "not observed".into(),
                    dirty: "not observed".into(),
                },
                OpOutcome::Failed { error },
            )
            .with_nothing_moved();
            self.record_op_impl(entry, cx, true, Some(display));
        } else {
            eprintln!("{op}: {error}");
            self.push_toast_for(
                super::toast_stack::ToastSubject::App,
                ToastKind::Error,
                display,
                cx,
            );
        }
        cx.notify();
    }
}
