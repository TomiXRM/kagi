use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use gpui::VisualTestAppContext;
use kagi_ui_core::i18n::{self, Lang, Msg};

pub fn scenario_push_tag_card_scope(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let language = i18n::lang();
    let fixture = build_fixture();
    let path = fixture.path();
    git(
        path,
        &["remote", "add", "origin", "https://fetch.example/repo.git"],
    );
    git(path, &["tag", "scope-light", "HEAD~1"]);
    git(
        path,
        &[
            "tag",
            "-a",
            "scope-annotated",
            "HEAD~1",
            "-m",
            "scope annotation",
        ],
    );
    let before = repo_fingerprint(path);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for (name, kind) in [
            ("scope-light", "lightweight"),
            ("scope-annotated", "annotated"),
        ] {
            git(path, &["config", "--replace-all", "remote.origin.pushurl", "https://secret-user:secret-token@publish.example/repo.git?token=secret-query#secret-fragment"]);
            git(
                path,
                &[
                    "config",
                    "--add",
                    "remote.origin.pushurl",
                    "ssh://secret-backup@backup.example/repo.git",
                ],
            );
            let (app, window) = mount(cx, path);
            app.update(cx, |app, cx| app.open_push_tag_modal(name.into(), cx));
            cx.run_until_parked();
            let identity = cx.read(|cx| {
                app.read(cx)
                    .push_tag_modal()
                    .unwrap()
                    .plan
                    .tag_push_identity
                    .clone()
                    .unwrap()
            });
            assert_ne!(identity.peeled_oid, before.0.trim());
            assert_eq!(identity.push_urls.len(), 2);
            // Drift the live configuration after approval. Display and Copy must
            // keep projecting the approved identity, never re-read the remote.
            git(
                path,
                &[
                    "config",
                    "--replace-all",
                    "remote.origin.pushurl",
                    "https://changed.example/unapproved.git",
                ],
            );
            crate::native_ax::activate(cx);
            cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
            let tree = crate::native_ax::tree(cx, window).to_string();
            assert!(
                tree.contains(&identity.object_oid),
                "Push tag card must expose the approved tag object OID: {tree}"
            );
            assert!(
                tree.contains(&identity.peeled_oid),
                "Push tag card must expose the peeled target OID"
            );
            assert!(
                tree.contains(kind),
                "Push tag card must expose the tag kind"
            );
            assert!(
                tree.contains("https://***@publish.example/repo.git?***#***"),
                "Push tag card must expose the redacted push URL, not the fetch URL: {tree}"
            );
            assert!(tree.contains("ssh://***@backup.example/repo.git"));
            assert!(tree.contains(match lang {
                Lang::En => "Tag to publish",
                Lang::Ja => "公開する tag",
            }));
            for secret in [
                "secret-user",
                "secret-token",
                "secret-query",
                "secret-fragment",
                "fetch.example",
                "secret-backup",
                "changed.example",
            ] {
                assert!(!tree.contains(secret), "AX leaks {secret}");
            }
            assert!(
                !tree.contains("plan-state-current"),
                "unrelated active branch must not be the main preview"
            );
            assert!(
                crate::native_ax::press(cx, window, Msg::ModalCopyAll.t()),
                "native Copy all must work"
            );
            cx.run_until_parked();
            let copied = cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap());
            for value in [
                &identity.object_oid,
                &identity.peeled_oid,
                name,
                kind,
                "https://***@publish.example/repo.git?***#***",
                "ssh://***@backup.example/repo.git",
            ] {
                assert!(copied.contains(value), "Copy all omits {value}: {copied}");
            }
            for secret in [
                "secret-user",
                "secret-token",
                "secret-query",
                "secret-fragment",
                "fetch.example",
                "secret-backup",
                "changed.example",
            ] {
                assert!(!copied.contains(secret), "Copy all leaks {secret}");
            }
            app.update(cx, |app, _| app.cancel_push_tag_modal());
            assert_eq!(
                repo_fingerprint(path),
                before,
                "scope preview and copy are read-only"
            );
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(language);
}
