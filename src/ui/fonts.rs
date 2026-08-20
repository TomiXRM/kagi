//! Startup registration for fonts embedded in the Kagi binary.

use gpui::App;

use super::CJK_FONT;

pub(super) fn load_bundled_fonts(cx: &mut App) {
    // Keep registration synchronous and before window creation so async views
    // inherit exactly the same available families as the initial render.
    if let Err(e) = cx.text_system().add_fonts(vec![
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Regular.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Bold.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        )),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf")),
        // Static Regular(400)/Bold(700) instances, not the variable font: on
        // Linux cosmic-text renders a *variable* fallback at its default axis,
        // and Noto Sans JP's default is Thin (wght=100), so Japanese looked
        // thin. Two static faces let fontdb resolve the requested weight
        // directly, exactly like the bundled Inter/JetBrains pair. (ADR-0130.)
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/NotoSansJP-Regular.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/NotoSansJP-Bold.ttf")),
    ]) {
        klog!("fonts: add_fonts failed (UI may fall back): {e}");
        return;
    }

    // Preserve the existing contract line.
    klog!("fonts: loaded Inter + JetBrains Mono");

    // The fallback check is opt-in: `all_font_names()` enumerates every font
    // installed on the machine, measured at 83-93ms on the main thread before
    // the first frame — 32% of a small repo's cold start — to print one line
    // nothing reads and no test asserts. Set `KAGI_FONT_DIAG=1` when
    // investigating a CJK rendering report (ADR-0130).
    if std::env::var("KAGI_FONT_DIAG").as_deref() != Ok("1") {
        return;
    }
    let cjk_ready = cx
        .text_system()
        .all_font_names()
        .iter()
        .any(|name| name == CJK_FONT);
    if cjk_ready {
        klog!("fonts: fallback {CJK_FONT}");
    } else {
        klog!("fonts: fallback missing {CJK_FONT}");
    }
}
