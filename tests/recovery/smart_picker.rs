//! Smart Commit gates against a real loopback Ollama fixture.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, MutexGuard};

// Preserve the request evidence even if a fixture thread panicked while
// recording it; poisoning must not hide the original scenario failure.
fn request_log(log: &Mutex<Vec<String>>) -> MutexGuard<'_, Vec<String>> {
    match log.lock() {
        Ok(log) => log,
        Err(poisoned) => poisoned.into_inner(),
    }
}

struct Ollama {
    host: String,
    generated: Arc<Mutex<Vec<String>>>,
    fail_generation: Arc<std::sync::atomic::AtomicBool>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    previous_host: Option<String>,
}
impl Ollama {
    fn new(models: &[String]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let tags = serde_json::json!({"models": models.iter().map(|name| serde_json::json!({"name": name})).collect::<Vec<_>>()}).to_string();
        let generated = Arc::new(Mutex::new(Vec::new()));
        let requests = generated.clone();
        let fail_generation = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let failing = fail_generation.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || {
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                let (header_end, length) = loop {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                while bytes.len() < header_end + length {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let generating = bytes.starts_with(b"POST /api/generate");
                let response = if generating {
                    let body: serde_json::Value =
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                    request_log(&requests).push(body["model"].as_str().unwrap().to_string());
                    serde_json::json!({"response": "Update staged fixture", "done": true})
                        .to_string()
                } else {
                    tags.clone()
                };
                let status = if generating && failing.load(std::sync::atomic::Ordering::Relaxed) {
                    "500 Internal Server Error"
                } else {
                    "200 OK"
                };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
        });
        let previous_host = std::env::var("KAGI_OLLAMA_HOST").ok();
        std::env::set_var("KAGI_OLLAMA_HOST", &host);
        Self {
            host,
            generated,
            fail_generation,
            stop,
            thread: Some(thread),
            previous_host,
        }
    }
}
impl Drop for Ollama {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
        if let Some(host) = &self.previous_host {
            std::env::set_var("KAGI_OLLAMA_HOST", host);
        } else {
            std::env::remove_var("KAGI_OLLAMA_HOST");
        }
    }
}
fn paint(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
    })
    .unwrap();
}
fn press(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(window, keystroke.clone());
    cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
}
pub fn keyboard(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::i18n::{self, Lang};
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "smart_commit_model",
        "smart_commit_llm_enabled",
        "lang",
    ]);
    let language = i18n::lang();
    for (count, lang, failure) in [
        (1, Lang::En, false),
        (40, Lang::Ja, false),
        (1, Lang::En, true),
        (1, Lang::Ja, true),
    ] {
        i18n::set_lang(lang);
        let models: Vec<_> = (0..count)
            .map(|i| format!("fixture-model-{i:02}-long-local-model-name"))
            .collect();
        let server = Ollama::new(&models);
        server
            .fail_generation
            .store(failure, std::sync::atomic::Ordering::Relaxed);
        let fixture = build_fixture();
        std::fs::write(fixture.path().join("smart.txt"), "staged fixture\n").unwrap();
        git(fixture.path(), &["add", "smart.txt"]);
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| {
            kagi::ui::e2e::open_local_panel_no_inputs(
                app,
                fixture.path().canonicalize().unwrap(),
                cx,
            );
            app.smart_commit.llm_enabled = true;
            app.smart_commit.provider = kagi::ui::smart_commit::SmartProvider::Ollama;
            app.smart_commit.model = None;
            app.smart_commit.detected_models =
                kagi_git::message_gen::ollama_list_models(&server.host);
            assert_eq!(
                app.smart_commit.detected_models, models,
                "fake Ollama tags must load"
            );
        });
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.smart_generate(app.active_session().unwrap(), window, cx)
            });
        })
        .unwrap();
        assert!(
            cx.read(|cx| app.read(cx).smart_commit_modal().is_some()),
            "Generate must open picker"
        );
        paint(cx, window);
        paint(cx, window);
        assert!(
            request_log(&server.generated).is_empty(),
            "opening rows must not generate"
        );
        // Opening the picker owns row focus; no pointer or focus seam is used.
        for key in ["down", "up", "end", "home", "pagedown", "pageup", "end"] {
            press(cx, window, key);
            paint(cx, window);
        }
        let focused = cx
            .update_window(window, |_, window, cx| {
                kagi::ui::e2e::smart_model_row(app.read(cx), window)
            })
            .unwrap()
            .expect("focused model must be mounted");
        assert_eq!(focused.0, count - 1);
        assert!(
            focused.1.top() >= focused.2.top() && focused.1.bottom() <= focused.2.bottom(),
            "navigation must reveal the entire selected row"
        );
        press(cx, window, "tab");
        paint(cx, window);
        assert!(
            cx.update_window(window, |_, window, cx| {
                kagi::ui::e2e::smart_model_row(app.read(cx), window)
            })
            .unwrap()
            .is_none(),
            "one Tab leaves the complete model list"
        );
        press(cx, window, "shift-tab");
        paint(cx, window);
        assert_eq!(
            cx.update_window(window, |_, window, cx| {
                kagi::ui::e2e::smart_model_row(app.read(cx), window).map(|row| row.0)
            })
            .unwrap(),
            Some(count - 1),
            "Shift+Tab returns to remembered visible row"
        );
        let ax = kagi::ui::list_a11y::recorded_list("smart-model-list").expect("named model list");
        assert_eq!(ax.role, Some(gpui::Role::ListBox));
        assert_eq!(ax.size, count);
        assert_eq!(
            ax.rows.get(&(count - 1)),
            Some(&(models.last().unwrap().clone(), true)),
            "focused destination must be drawn and AX-selected with its complete name"
        );
        press(cx, window, if count == 1 { "enter" } else { "space" });
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| app.read(cx).smart_commit.model.clone()),
            models.last().cloned(),
            "keyboard must persist focused model"
        );
        // ADR-0090 retries a quick provider rejection once without `think`.
        let expected_requests = if failure { 2 } else { 1 };
        assert_eq!(
            *request_log(&server.generated),
            vec![models.last().unwrap().clone(); expected_requests],
            "one generation must use the saved model, including the existing rejection retry"
        );
        assert!(cx.read(|cx| app.read(cx).smart_commit_modal().is_none()));
        let expected_status = match (lang, failure) {
            (Lang::En, false) => "Generated with local LLM",
            (Lang::Ja, false) => "ローカルLLMで生成しました",
            (Lang::En, true) => "LLM unavailable — used rule-based",
            (Lang::Ja, true) => "LLMを利用できません — ルールベースで生成しました",
        };
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().smart_commit_status.clone())
                .as_deref(),
            Some(expected_status),
            "post-generation status must follow the UI language, including HTTP failure fallback"
        );
        // A second opening cancelled by Escape must not generate or save.
        app.update(cx, |app, _| app.smart_commit.model = None);
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.smart_generate(app.active_session().unwrap(), window, cx)
            });
        })
        .unwrap();
        paint(cx, window);
        paint(cx, window);
        press(cx, window, "escape");
        assert_eq!(request_log(&server.generated).len(), expected_requests);
        assert!(cx.read(|cx| app.read(cx).smart_commit.model.is_none()
            && app.read(cx).smart_commit_modal().is_none()));
        unmount(cx, app, window);
    }
    i18n::set_lang(language);
}

pub fn empty(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::i18n::{self, Lang};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["smart_commit_llm_enabled", "lang"]);
    let language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        let server = Ollama::new(&[]);
        let fixture = build_fixture();
        std::fs::write(fixture.path().join("smart.txt"), "staged fixture\n").unwrap();
        git(fixture.path(), &["add", "smart.txt"]);
        let repo = fixture.path().canonicalize().unwrap();
        let backend = kagi_git::Backend::open(&repo).unwrap();
        let expected = kagi_git::message_gen::rule_based(
            &kagi_git::message_gen::GenInput {
                diff: String::new(),
                lang: if lang == Lang::Ja {
                    kagi_git::message_gen::Lang::Ja
                } else {
                    kagi_git::message_gen::Lang::En
                },
                style: kagi_git::message_gen::Style::Plain,
                want_body: true,
            },
            &backend.collect_staged_files(),
        );
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            kagi::ui::e2e::open_local_panel_no_inputs(app, repo.clone(), cx);
            app.smart_commit.provider = kagi::ui::smart_commit::SmartProvider::Ollama;
            app.smart_commit.model = None;
            app.smart_commit.detected_models =
                kagi_git::message_gen::ollama_list_models(&server.host);
            assert!(app.smart_commit.detected_models.is_empty());
            app.smart_commit.lang = if lang == Lang::Ja {
                kagi_git::message_gen::Lang::Ja
            } else {
                kagi_git::message_gen::Lang::En
            };
        });
        // Both first-time consent and already-enabled retry replace a nonempty draft.
        for consent in [true, false] {
            app.update(cx, |app, cx| {
                app.smart_commit.llm_enabled = !consent;
                app.ui()
                    .commit_panel
                    .clone()
                    .unwrap()
                    .update(cx, |panel, _| {
                        panel.state.commit_msg = "existing draft must be replaced".to_string();
                    });
            });
            let revision = cx.read(|cx| {
                app.read(cx)
                    .ui()
                    .commit_panel
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .gen
            });
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| {
                    app.smart_generate(app.active_session().unwrap(), window, cx)
                });
            })
            .unwrap();
            if consent {
                assert!(cx.read(|cx| app.read(cx).smart_commit_modal().is_some()));
                paint(cx, window);
                press(cx, window, "enter");
            }
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.smart_commit_modal().is_none());
                assert_eq!(
                    state.ui().commit_panel.as_ref().unwrap().read(cx).gen,
                    revision.wrapping_add(1),
                    "one request inserts exactly one fallback draft"
                );
                assert_eq!(
                    state
                        .ui()
                        .commit_panel
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .state
                        .commit_msg,
                    expected,
                    "empty-model fallback must actually replace the draft"
                );
                assert_eq!(
                    state.ui().smart_commit_status.as_deref(),
                    Some(if lang == Lang::Ja {
                        "ローカルモデルがありません — ルールベースの下書きを挿入しました"
                    } else {
                        "No local models found — rule-based draft inserted"
                    })
                );
            });
            assert!(
                request_log(&server.generated).is_empty(),
                "empty fallback sends no generation request"
            );
        }
        // A rule-based fallback supersedes a previously pending LLM response.
        let timer = cx.background_executor.clone();
        kagi::ui::e2e::queue_smart_generation(cx.background_executor.spawn(async move {
            timer.timer(std::time::Duration::from_secs(1)).await;
            Some(("old LLM response must not reapply".to_string(), true))
        }));
        app.update(cx, |app, _| {
            app.smart_commit.model = Some("previous-model".to_string())
        });
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                let owner = app.active_session().unwrap();
                app.smart_generate(owner, window, cx);
                app.smart_commit.model = None;
                app.smart_generate(owner, window, cx);
            });
        })
        .unwrap();
        cx.advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(
                app.read(cx)
                    .ui()
                    .commit_panel
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .state
                    .commit_msg,
                expected,
                "a superseded generation cannot double-apply over the rule-based fallback"
            )
        });
        // A departed consent owner must not replace the background or active draft.
        app.update(cx, |app, _| app.smart_commit.llm_enabled = false);
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.smart_generate(app.active_session().unwrap(), window, cx)
            });
        })
        .unwrap();
        let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
        let panel_a = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
        let other = build_fixture();
        app.update(cx, |app, cx| {
            assert!(app.open_repository(other.path().canonicalize().unwrap(), cx));
            kagi::ui::e2e::open_local_panel_no_inputs(
                app,
                other.path().canonicalize().unwrap(),
                cx,
            );
            app.ui()
                .commit_panel
                .clone()
                .unwrap()
                .update(cx, |panel, _| {
                    panel.state.commit_msg = "B draft".to_string()
                });
        });
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.smart_generate(owner, window, cx);
                app.confirm_smart_consent(window, cx);
            });
        })
        .unwrap();
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state
                    .ui()
                    .commit_panel
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .state
                    .commit_msg,
                "B draft"
            );
            assert!(state.ui().smart_commit_status.is_none());
            assert_eq!(panel_a.read(cx).state.commit_msg, expected);
        });
        drop(panel_a);
        unmount(cx, app, window);
    }
    i18n::set_lang(language);
}
