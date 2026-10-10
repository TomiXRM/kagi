//! Smart Commit gates against a real loopback Ollama fixture.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use parking_lot::Mutex;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

struct Ollama {
    host: String,
    generated: Arc<Mutex<Vec<String>>>,
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
                    requests
                        .lock()
                        .push(body["model"].as_str().unwrap().to_string());
                    serde_json::json!({"response": "Update staged fixture", "done": true})
                        .to_string()
                } else {
                    tags.clone()
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
        });
        let previous_host = std::env::var("KAGI_OLLAMA_HOST").ok();
        std::env::set_var("KAGI_OLLAMA_HOST", &host);
        Self {
            host,
            generated,
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
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["smart_commit_model", "smart_commit_llm_enabled"]);
    for count in [1, 40] {
        let models: Vec<_> = (0..count)
            .map(|i| format!("fixture-model-{i:02}-long-local-model-name"))
            .collect();
        let server = Ollama::new(&models);
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
            server.generated.lock().is_empty(),
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
        assert_eq!(
            *server.generated.lock(),
            vec![models.last().unwrap().clone()],
            "saved model must match exactly one actual generation"
        );
        assert!(cx.read(|cx| app.read(cx).smart_commit_modal().is_none()));
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
        assert_eq!(server.generated.lock().len(), 1);
        assert!(cx.read(|cx| app.read(cx).smart_commit.model.is_none()
            && app.read(cx).smart_commit_modal().is_none()));
        unmount(cx, app, window);
    }
}
