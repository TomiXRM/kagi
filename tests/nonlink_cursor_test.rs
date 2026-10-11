//! #1241: native action renderers must not request the URL-link cursor.
//! GPUI's rendered-frame cursor observation is crate-private, so guard the
//! production element-tree builders rather than introducing a test-only cursor.
#[test]
fn native_action_element_trees_do_not_request_pointing_hand() {
    for (name, source) in [
        (
            "commit_panel",
            include_str!("../src/ui/commit_panel_render.rs"),
        ),
        ("menu", include_str!("../src/ui/menu_overlay.rs")),
        ("hunk", include_str!("../src/ui/diff_view/hunk_action.rs")),
        ("home", include_str!("../src/ui/home.rs")),
        ("toolbar", include_str!("../src/ui/render_header.rs")),
    ] {
        assert!(
            !source.contains("PointingHand"),
            "{name}: link cursor on action"
        );
        assert!(
            !source.contains("cursor_pointer()"),
            "{name}: link cursor on action"
        );
    }
}
