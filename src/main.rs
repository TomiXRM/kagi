mod git;

use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

use git::open_repository;

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
                let details = vec![
                    format!("path: {}", info.workdir.display()),
                    format!("HEAD: {}", info.head.display()),
                ];
                // Log to stderr so automated tests can verify output without
                // needing to inspect the GPU window.
                eprintln!("[kagi] {}", msg);
                for d in &details {
                    eprintln!("[kagi] {}", d);
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
