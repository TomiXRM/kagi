//! ADR-0166 — GUI E2E main-thread runner (Lane 1, macOS native).
//!
//! This is the GREEN path the PoC (`#[ignore]`d unit test) could only gesture
//! at. It is a `harness = false` test target (see `Cargo.toml`): it owns
//! `fn main`, so it runs on the **process main thread** — where AppKit
//! `NSWindow` creation succeeds. libtest's default harness runs `#[test]` fns on
//! worker threads, where the same window creation SIGABRTs; that is the sole
//! reason the PoC stayed `#[ignore]`d.
//!
//! It links the `kagi` lib (which now hosts `ui`, ADR-0166) and drives the REAL
//! `KagiApp` root through `gpui::VisualTestAppContext` (unlocked by the
//! explicit `gui-e2e` feature; default builds leave `test-support` disabled).
//! Every normal-dep detail (`gpui_component`, `gpui_platform`, `kagi_git`) is
//! hidden behind the `kagi::ui::e2e` seam, so this file touches only `gpui`
//! (normal dependency), `image`/`tempfile` (dev-deps), and `kagi`.
//!
//! Each scenario mounts the real root, drives it (keystroke / registered action
//! / entity update), settles deterministically, and asserts observable state —
//! `KagiApp` fields, the clipboard, or repo refs (ADR-0166 §3: the assertions
//! are the oracle, screenshots are triage only). Coverage: bottom-panel toggle
//! (PoC), graph Cmd+C copy (ADR-0170), Create Snapshot (#335), command-palette
//! theme switch (#373), agent provenance (#337), WIP→HEAD connectors (#472),
//! and the linked-worktree commit panel's writes (#473 / #476 slices 1–3).
//! Exits 0 on success, non-zero (panic → 101) on failure.
//!
//! Run (opt-in — see the `KAGI_GUI_E2E` guard in `run`):
//!   KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY=remote_refresh_departed_owner \
//!     cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
//!
//! Default workspace tests omit this target (including its recovery modules).
//! With `gui-e2e` but without `KAGI_GUI_E2E`, it still prints SKIP and exits 0.
//!
//! Native image capture is enabled only by `gui-e2e` through the platform's
//! test-support feature. Paint regressions can assert Metal-rendered pixels;
//! screenshots do not replace state/safety assertions (ADR-0166 §3).

#[cfg(not(target_os = "macos"))]
fn main() {
    // The visual driver is macOS-only (real Metal + AppKit). Elsewhere this
    // target is a no-op success so `cargo test --workspace` stays green.
    eprintln!("[gui-e2e] SKIP: VisualTestAppContext is macOS-only");
}

#[cfg(target_os = "macos")]
#[path = "recovery/batch_staging_refresh.rs"]
mod batch_staging_refresh;
#[cfg(target_os = "macos")]
#[path = "recovery/time_locale.rs"]
mod time_locale;

#[cfg(target_os = "macos")]
#[path = "recovery/open_picker.rs"]
mod open_picker;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_open.rs"]
mod conflict_open;

#[cfg(target_os = "macos")]
#[path = "gui_e2e/side_pair.rs"]
mod side_pair;

#[cfg(target_os = "macos")]
#[path = "recovery/ref_validation.rs"]
mod ref_validation;

#[cfg(target_os = "macos")]
#[path = "recovery/squash_safe_default.rs"]
mod squash_safe_default;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_dashboard_contrast.rs"]
mod pr_dashboard_contrast;
#[cfg(target_os = "macos")]
#[path = "recovery/pr_dashboard_hover_selected.rs"]
mod pr_dashboard_hover_selected;
#[cfg(target_os = "macos")]
#[path = "recovery/pr_dashboard_keyboard.rs"]
mod pr_dashboard_keyboard;
#[cfg(target_os = "macos")]
#[path = "recovery/surface_scrollbars.rs"]
mod surface_scrollbars;
#[cfg(target_os = "macos")]
fn main() {
    std::process::exit(macos::run());
}

#[cfg(target_os = "macos")]
#[path = "recovery/file_tree_a11y.rs"]
mod file_tree_a11y;
#[cfg(target_os = "macos")]
#[path = "support/gui_evidence.rs"]
mod gui_evidence;
#[cfg(target_os = "macos")]
#[path = "support/gui_isolation.rs"]
mod gui_isolation;
#[cfg(target_os = "macos")]
#[path = "recovery/pr_detail_a11y.rs"]
mod pr_detail_a11y;
#[cfg(target_os = "macos")]
#[path = "recovery/pr_lane_keys.rs"]
mod pr_lane_keys;
#[cfg(target_os = "macos")]
#[path = "recovery/presentation_support.rs"]
mod presentation_support;
#[cfg(target_os = "macos")]
#[path = "support/proc_identity.rs"]
mod proc_identity;
#[cfg(target_os = "macos")]
#[path = "recovery/editor_tree_motion.rs"]
mod recovery_editor_tree_motion;
#[cfg(target_os = "macos")]
#[path = "recovery/sidebar_tree_motion.rs"]
mod recovery_sidebar_tree_motion;
#[cfg(target_os = "macos")]
#[path = "support/repository_state.rs"]
mod repository_state;
#[cfg(target_os = "macos")]
#[path = "recovery/theme_custom.rs"]
mod theme_custom;

#[cfg(target_os = "macos")]
#[path = "recovery/menus_window.rs"]
mod menus_window;

#[cfg(target_os = "macos")]
#[path = "recovery/menus_ja.rs"]
mod menus_ja;

#[cfg(target_os = "macos")]
#[path = "recovery/plan_labels.rs"]
mod plan_labels;

#[cfg(target_os = "macos")]
#[path = "recovery/plan_card_reachability.rs"]
mod plan_card_reachability;

#[cfg(target_os = "macos")]
#[path = "recovery/rebase_card_scope.rs"]
mod rebase_card_scope;

#[cfg(target_os = "macos")]
#[path = "recovery/push_tag_card_scope.rs"]
mod push_tag_card_scope;

#[cfg(target_os = "macos")]
#[path = "recovery/toast_dismiss.rs"]
mod toast_dismiss;

#[cfg(target_os = "macos")]
#[path = "recovery/toast_stack_budget.rs"]
mod toast_stack_budget;

#[cfg(target_os = "macos")]
#[path = "recovery/feedback_status.rs"]
mod feedback_status;

#[cfg(target_os = "macos")]
#[path = "recovery/clone_hidden_status.rs"]
mod clone_hidden_status;
#[cfg(target_os = "macos")]
#[path = "recovery/clone_progress_status.rs"]
mod clone_progress_status;
#[cfg(target_os = "macos")]
#[path = "recovery/completion_visit_owner.rs"]
mod completion_visit_owner;
#[cfg(target_os = "macos")]
#[path = "recovery/remote_pull_result_owner.rs"]
mod remote_pull_result_owner;
#[cfg(target_os = "macos")]
#[path = "recovery/remote_result_refresh.rs"]
mod remote_result_refresh;

#[cfg(target_os = "macos")]
#[path = "recovery/worktree_inspection_actions.rs"]
mod worktree_inspection_actions;

#[cfg(target_os = "macos")]
#[path = "recovery/system_accent.rs"]
mod system_accent;

#[cfg(target_os = "macos")]
#[path = "recovery/system_accessibility.rs"]
mod system_accessibility;

#[cfg(target_os = "macos")]
#[path = "recovery/settings_quiet.rs"]
mod settings_quiet;

#[cfg(target_os = "macos")]
#[path = "recovery/settings_composition.rs"]
mod settings_composition;
#[cfg(target_os = "macos")]
#[path = "recovery/settings_window.rs"]
mod settings_window;

#[cfg(target_os = "macos")]
#[path = "recovery/window_restore.rs"]
mod window_restore;

#[cfg(target_os = "macos")]
#[path = "recovery/window_min_size.rs"]
mod window_min_size;

#[cfg(target_os = "macos")]
#[path = "recovery/close_window_initial_home.rs"]
mod close_window_initial_home;
#[cfg(target_os = "macos")]
#[path = "recovery/help_shortcuts_glyphs.rs"]
mod help_shortcuts_glyphs;
#[cfg(target_os = "macos")]
#[path = "recovery/shortcuts_panel_scroll.rs"]
mod shortcuts_panel_scroll;
#[cfg(target_os = "macos")]
#[path = "recovery/tab_restore_active_identity.rs"]
mod tab_restore_active_identity;

#[cfg(target_os = "macos")]
#[path = "recovery/primary_button.rs"]
mod primary_button;

#[cfg(target_os = "macos")]
#[path = "recovery/modal_count_contrast.rs"]
mod modal_count_contrast;

#[cfg(target_os = "macos")]
#[path = "recovery/modal_scale.rs"]
mod modal_scale;

#[cfg(target_os = "macos")]
#[path = "recovery/modal_note_paths.rs"]
mod modal_note_paths;

#[cfg(target_os = "macos")]
#[path = "recovery/dark_elevation.rs"]
mod dark_elevation;

#[cfg(target_os = "macos")]
#[path = "recovery/inactive_selection.rs"]
mod inactive_selection;

#[cfg(target_os = "macos")]
#[path = "recovery/selection_on_accent.rs"]
mod selection_on_accent;

#[cfg(target_os = "macos")]
#[path = "recovery/graph_selection_wash.rs"]
mod graph_selection_wash;

#[cfg(target_os = "macos")]
#[path = "recovery/graph_motion_edges.rs"]
mod graph_motion_edges;

#[cfg(target_os = "macos")]
#[path = "recovery/tab_strip.rs"]
mod tab_strip;

#[cfg(target_os = "macos")]
#[path = "recovery/operations.rs"]
mod recovery_operations;

#[cfg(target_os = "macos")]
#[path = "recovery/sync_layout.rs"]
mod sync_layout;

#[cfg(target_os = "macos")]
#[path = "recovery/update_notice.rs"]
mod update_notice;

#[cfg(target_os = "macos")]
#[path = "recovery/create_branch_warnings.rs"]
mod create_branch_warnings;

#[cfg(target_os = "macos")]
#[path = "recovery/create_branch_cancel_reopen.rs"]
mod create_branch_cancel_reopen;

#[cfg(target_os = "macos")]
#[path = "recovery/create_branch_feedback.rs"]
mod create_branch_feedback;

#[cfg(target_os = "macos")]
#[path = "recovery/smart_picker.rs"]
mod smart_picker;

#[cfg(target_os = "macos")]
#[path = "recovery/smart_commit_settings_consent.rs"]
mod smart_commit_settings_consent;

#[cfg(target_os = "macos")]
#[path = "recovery/sidebar_panes.rs"]
mod recovery_sidebar_panes;

#[cfg(target_os = "macos")]
#[path = "recovery/dnd_self.rs"]
mod dnd_self;

#[cfg(target_os = "macos")]
#[path = "recovery/sidebar_filter.rs"]
mod sidebar_filter;
#[cfg(target_os = "macos")]
#[path = "recovery/sidebar_search.rs"]
mod sidebar_search;

#[cfg(target_os = "macos")]
#[path = "recovery/graph_focus.rs"]
mod graph_focus;

#[cfg(target_os = "macos")]
#[path = "recovery/input_names.rs"]
mod input_names;

#[cfg(target_os = "macos")]
#[path = "recovery/input_context_menus.rs"]
mod input_context_menus;

#[cfg(target_os = "macos")]
#[path = "recovery/rename_form_state.rs"]
mod rename_form_state;

#[cfg(target_os = "macos")]
#[path = "recovery/hairline_dividers.rs"]
mod recovery_hairline_dividers;

#[cfg(target_os = "macos")]
#[path = "recovery/bottom_panel.rs"]
mod recovery_bottom_panel;

#[cfg(target_os = "macos")]
#[path = "recovery/activity_a11y.rs"]
mod activity_a11y;
#[cfg(target_os = "macos")]
#[path = "recovery/activity_bucket_hit.rs"]
mod activity_bucket_hit;

#[cfg(target_os = "macos")]
#[path = "recovery/terminal_states.rs"]
mod recovery_terminal_states;

#[cfg(target_os = "macos")]
#[path = "recovery/terminal_keys.rs"]
mod recovery_terminal_keys;

#[cfg(target_os = "macos")]
#[path = "recovery/terminal_fonts.rs"]
mod recovery_terminal_fonts;

#[cfg(target_os = "macos")]
#[path = "recovery/unborn_empty.rs"]
mod recovery_unborn_empty;

#[cfg(target_os = "macos")]
#[path = "recovery/bottom_panel_motion.rs"]
mod bottom_panel_motion;

#[cfg(target_os = "macos")]
#[path = "recovery/bottom_panel_divider_drag.rs"]
mod bottom_panel_divider_drag;

#[cfg(target_os = "macos")]
#[path = "recovery/graph_column_divider.rs"]
mod graph_column_divider;
#[cfg(target_os = "macos")]
#[path = "recovery/pane_budget.rs"]
mod pane_budget;
#[cfg(target_os = "macos")]
#[path = "recovery/side_panel_motion.rs"]
mod side_panel_motion;

#[cfg(target_os = "macos")]
#[path = "recovery/hunk_staging.rs"]
mod hunk_staging;

#[cfg(target_os = "macos")]
#[path = "recovery/diff_keyboard.rs"]
mod diff_keyboard;

#[cfg(target_os = "macos")]
#[path = "recovery/diff_line_ending_annotation.rs"]
mod diff_line_ending_annotation;
#[cfg(target_os = "macos")]
#[path = "recovery/diff_selection_revision.rs"]
mod diff_selection_revision;
#[cfg(target_os = "macos")]
#[path = "recovery/diff_split_unicode_badge.rs"]
mod diff_split_unicode_badge;

#[cfg(target_os = "macos")]
#[path = "recovery/reconcile_unobservable.rs"]
mod reconcile_unobservable;

#[cfg(target_os = "macos")]
#[path = "recovery/pull.rs"]
mod recovery_pull;

#[cfg(target_os = "macos")]
#[path = "recovery/pull_freshness.rs"]
mod pull_freshness;

#[cfg(target_os = "macos")]
#[path = "recovery/entry_consistency.rs"]
mod entry_consistency;

#[cfg(target_os = "macos")]
#[path = "recovery/pull_fetch_blocker.rs"]
mod pull_fetch_blocker;

#[cfg(target_os = "macos")]
#[path = "recovery/cli_capability.rs"]
mod recovery_cli_capability;

#[cfg(target_os = "macos")]
#[path = "recovery/app_remove.rs"]
mod app_remove;

#[cfg(target_os = "macos")]
#[path = "recovery/app_stash.rs"]
mod app_stash;

#[cfg(target_os = "macos")]
#[path = "recovery/stash_entry_identity.rs"]
mod stash_entry_identity;

#[cfg(target_os = "macos")]
#[path = "recovery/stash_apply_manifest.rs"]
mod stash_apply_manifest;

#[cfg(target_os = "macos")]
#[path = "recovery/busy_label.rs"]
mod busy_label;

#[cfg(target_os = "macos")]
#[path = "recovery/fetch_recording_failure.rs"]
mod fetch_recording_failure;

#[cfg(target_os = "macos")]
#[path = "recovery/user_text.rs"]
mod user_text;

#[cfg(target_os = "macos")]
#[path = "recovery/busy_dismiss.rs"]
mod busy_dismiss;
#[cfg(target_os = "macos")]
#[path = "recovery/busy_failure.rs"]
mod busy_failure;
#[cfg(target_os = "macos")]
#[path = "recovery/op_queue.rs"]
mod op_queue;
#[cfg(target_os = "macos")]
#[path = "recovery/slow_read.rs"]
mod slow_read;

#[cfg(target_os = "macos")]
#[path = "recovery/app_writer_admission.rs"]
mod app_writer_admission;

#[cfg(target_os = "macos")]
#[path = "recovery/app_conflict.rs"]
mod app_conflict;

#[cfg(target_os = "macos")]
#[path = "recovery/repository_lock.rs"]
mod repository_lock;

#[cfg(target_os = "macos")]
#[path = "recovery/home_heading.rs"]
mod home_heading;
#[cfg(target_os = "macos")]
#[path = "recovery/home_recent_remove.rs"]
mod home_recent_remove;
#[cfg(target_os = "macos")]
#[path = "recovery/home_tab.rs"]
mod home_tab;

#[cfg(target_os = "macos")]
#[path = "recovery/branch_copy.rs"]
mod branch_copy;
#[cfg(target_os = "macos")]
#[path = "recovery/branch_menu_compact.rs"]
mod branch_menu_compact;
#[cfg(target_os = "macos")]
#[path = "recovery/clone_card.rs"]
mod clone_card;
#[cfg(target_os = "macos")]
#[path = "recovery/clone_menu.rs"]
mod clone_menu;
#[cfg(target_os = "macos")]
#[path = "recovery/clone_owner.rs"]
mod clone_owner;
#[cfg(target_os = "macos")]
#[path = "recovery/commit_paging.rs"]
mod commit_paging;
#[cfg(target_os = "macos")]
#[path = "recovery/context_menu_keys.rs"]
mod context_menu_keys;
#[cfg(target_os = "macos")]
#[path = "recovery/graph_tab_stop.rs"]
mod graph_tab_stop;
#[cfg(target_os = "macos")]
#[path = "recovery/home_empty_section_wording.rs"]
mod home_empty_section_wording;
#[cfg(target_os = "macos")]
#[path = "recovery/home_github.rs"]
mod home_github;
#[cfg(target_os = "macos")]
#[path = "recovery/home_list_place.rs"]
mod home_list_place;
#[cfg(target_os = "macos")]
#[path = "recovery/home_p2.rs"]
mod home_p2;
#[cfg(target_os = "macos")]
#[path = "recovery/home_rows.rs"]
mod home_rows;
#[cfg(target_os = "macos")]
#[path = "recovery/home_search_incomplete.rs"]
mod home_search_incomplete;
#[cfg(target_os = "macos")]
#[path = "recovery/home_work.rs"]
mod home_work;
#[cfg(target_os = "macos")]
#[path = "recovery/keyboard_nav.rs"]
mod keyboard_nav;
#[cfg(target_os = "macos")]
#[path = "recovery/lists_keyboard.rs"]
mod lists_keyboard;
#[cfg(target_os = "macos")]
#[path = "recovery/state_labels.rs"]
mod state_labels;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_abort_slot.rs"]
mod conflict_abort_slot;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_continue_cache.rs"]
mod conflict_continue_cache;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_draft_revision.rs"]
mod conflict_draft_revision;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_draft_barrier_edges.rs"]
mod conflict_draft_barrier_edges;
#[cfg(target_os = "macos")]
#[path = "recovery/conflict_draft_persist_failure.rs"]
mod conflict_draft_persist_failure;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_continue_approval.rs"]
mod conflict_continue_approval;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_draft_inspection.rs"]
mod conflict_draft_inspection;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_stopped.rs"]
mod conflict_stopped;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_controls.rs"]
mod conflict_controls;
#[cfg(target_os = "macos")]
#[path = "recovery/conflict_file_selection.rs"]
mod conflict_file_selection;
#[cfg(target_os = "macos")]
#[path = "recovery/conflict_marker_width.rs"]
mod conflict_marker_width;
#[cfg(target_os = "macos")]
#[path = "recovery/conflict_text_keyboard.rs"]
mod conflict_text_keyboard;

#[cfg(target_os = "macos")]
#[path = "recovery/repo_health.rs"]
mod repo_health;

#[cfg(target_os = "macos")]
#[path = "recovery/analyze_a11y.rs"]
mod analyze_a11y;

#[cfg(target_os = "macos")]
#[path = "recovery/modal_polish_b.rs"]
mod modal_polish_b;

#[cfg(target_os = "macos")]
#[path = "recovery/destructive_modal_return.rs"]
mod destructive_modal_return;

#[cfg(target_os = "macos")]
#[path = "recovery/inventory/mod.rs"]
mod inventory;

#[cfg(target_os = "macos")]
#[path = "recovery/worktree_folder_picker.rs"]
mod worktree_folder_picker;

#[cfg(target_os = "macos")]
#[path = "recovery/worktree_default_name.rs"]
mod worktree_default_name;

#[cfg(target_os = "macos")]
#[path = "recovery/refusal_reasons.rs"]
mod refusal_reasons;

#[cfg(target_os = "macos")]
#[path = "recovery/diff_highlight.rs"]
mod diff_highlight;

#[cfg(target_os = "macos")]
#[path = "recovery/binary_diff_owner.rs"]
mod binary_diff_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/commit_pdf.rs"]
mod commit_pdf;

#[cfg(target_os = "macos")]
#[path = "recovery/pdf_sync.rs"]
mod pdf_sync;

#[cfg(target_os = "macos")]
#[path = "recovery/pdf_fit.rs"]
mod pdf_fit;

#[cfg(target_os = "macos")]
#[path = "recovery/pdf_focus_retirement.rs"]
mod pdf_focus_retirement;

#[cfg(target_os = "macos")]
#[path = "recovery/pdf_media_bounds.rs"]
mod pdf_media_bounds;

#[cfg(target_os = "macos")]
#[path = "recovery/modal_viewports.rs"]
mod modal_viewports;
#[cfg(target_os = "macos")]
#[path = "recovery/remote_browse_focus.rs"]
mod remote_browse_focus;

#[cfg(target_os = "macos")]
#[path = "recovery/native_ax.rs"]
mod native_ax;
#[cfg(target_os = "macos")]
#[path = "recovery/overlay_focus.rs"]
mod overlay_focus;
#[cfg(target_os = "macos")]
#[path = "recovery/palette_ax.rs"]
mod palette_ax;
#[cfg(target_os = "macos")]
#[path = "recovery/palette_polish.rs"]
mod palette_polish;
#[cfg(target_os = "macos")]
#[path = "recovery/settings_analyze.rs"]
mod settings_analyze;
#[cfg(target_os = "macos")]
#[path = "recovery/settings_appearance_ax.rs"]
mod settings_appearance_ax;
#[cfg(target_os = "macos")]
#[path = "recovery/settings_controls.rs"]
mod settings_controls;
#[cfg(target_os = "macos")]
#[path = "recovery/settings_switches.rs"]
mod settings_switches;
#[cfg(target_os = "macos")]
#[path = "recovery/sidebar_groups.rs"]
mod sidebar_groups;
#[cfg(target_os = "macos")]
#[path = "recovery/sidebar_identity.rs"]
mod sidebar_identity;
#[cfg(target_os = "macos")]
#[path = "recovery/sidebar_rows.rs"]
mod sidebar_rows;
#[cfg(target_os = "macos")]
#[path = "recovery/tab_panels.rs"]
mod tab_panels;
#[cfg(target_os = "macos")]
#[path = "recovery/zoom_200.rs"]
mod zoom_200;

#[cfg(target_os = "macos")]
#[path = "recovery/status_bar_fit.rs"]
mod status_bar_fit;
#[cfg(target_os = "macos")]
#[path = "recovery/status_chips.rs"]
mod status_chips;
#[cfg(target_os = "macos")]
#[path = "recovery/status_controls.rs"]
mod status_controls;
#[cfg(target_os = "macos")]
#[path = "recovery/titlebar_tabs.rs"]
mod titlebar_tabs;
#[cfg(target_os = "macos")]
#[path = "recovery/toolbar_overflow.rs"]
mod toolbar_overflow;

#[cfg(target_os = "macos")]
#[path = "recovery/toolbar_overflow_priority.rs"]
mod toolbar_overflow_priority;

#[cfg(target_os = "macos")]
#[path = "recovery/toolbar_pr_sidebar.rs"]
mod toolbar_pr_sidebar;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_fields_focus.rs"]
mod pr_fields_focus;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_same_number.rs"]
mod pr_same_number;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_viewed.rs"]
mod pr_viewed;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_diff_extent.rs"]
mod pr_diff_extent;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_detail_adaptive.rs"]
mod pr_detail_adaptive;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_diff_extent_invalidation.rs"]
mod pr_diff_extent_invalidation;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_diff_extent_same_selection.rs"]
mod pr_diff_extent_same_selection;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_diff_extent_reload.rs"]
mod editor_diff_extent_reload;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_pdf.rs"]
mod editor_pdf;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_pdf_keyboard.rs"]
mod editor_pdf_keyboard;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_pdf_scrollbars.rs"]
mod editor_pdf_scrollbars;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_details.rs"]
mod editor_details;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_tabs.rs"]
mod editor_tabs;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_find.rs"]
mod editor_find;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_find_hosts.rs"]
mod editor_find_hosts;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_threads.rs"]
mod pr_threads;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_threads_projection.rs"]
mod pr_threads_projection;

#[cfg(target_os = "macos")]
#[path = "recovery/github_text.rs"]
mod github_text;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_suggestion_apply.rs"]
mod pr_suggestion_apply;

#[cfg(target_os = "macos")]
#[path = "recovery/layout.rs"]
mod recovery_layout;

#[cfg(target_os = "macos")]
#[path = "recovery/file_history_open.rs"]
mod file_history_open;
#[cfg(target_os = "macos")]
#[path = "recovery/inspector_history_keyboard.rs"]
mod inspector_history_keyboard;

#[cfg(target_os = "macos")]
#[path = "recovery/commit_panel_density.rs"]
mod commit_panel_density;

#[cfg(target_os = "macos")]
#[path = "recovery/changes_motion.rs"]
mod changes_motion;

#[cfg(target_os = "macos")]
#[path = "recovery/inspector_motion.rs"]
mod inspector_motion;

#[cfg(target_os = "macos")]
#[path = "recovery/file_row_input.rs"]
mod file_row_input;

#[cfg(target_os = "macos")]
#[path = "recovery/modal_motion.rs"]
mod modal_motion;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_snapshot_readonly.rs"]
mod editor_snapshot_readonly;

#[cfg(target_os = "macos")]
#[path = "recovery/editor_history_error.rs"]
mod editor_history_error;

#[cfg(target_os = "macos")]
#[path = "recovery/blame_identity.rs"]
mod blame_identity;

#[cfg(target_os = "macos")]
#[path = "recovery/blame_toggle.rs"]
mod blame_toggle;

#[cfg(target_os = "macos")]
#[path = "recovery/header_fit.rs"]
mod recovery_header_fit;

#[cfg(target_os = "macos")]
#[path = "recovery/diff_presentation.rs"]
mod recovery_diff_presentation;

#[cfg(target_os = "macos")]
#[path = "recovery/commit_panel_viewport.rs"]
mod recovery_commit_panel_viewport;

#[cfg(target_os = "macos")]
#[path = "recovery/compact_layout.rs"]
mod compact_layout;

#[cfg(target_os = "macos")]
#[path = "recovery/compact_rail.rs"]
mod compact_rail;

#[cfg(target_os = "macos")]
#[path = "recovery/compact_select_changes.rs"]
mod compact_select_changes;

#[cfg(target_os = "macos")]
#[path = "recovery/graph_return_changes.rs"]
mod graph_return_changes;

#[cfg(target_os = "macos")]
#[path = "recovery/commit_panel_i18n.rs"]
mod recovery_commit_panel_i18n;

#[cfg(target_os = "macos")]
#[path = "recovery/commit_panel_polish.rs"]
mod recovery_commit_panel_polish;

#[cfg(target_os = "macos")]
#[path = "recovery/commit_panel_keyboard.rs"]
mod recovery_commit_panel_keyboard;

#[cfg(target_os = "macos")]
#[path = "recovery/commit_panel_file.rs"]
mod recovery_commit_panel_file;

#[cfg(target_os = "macos")]
#[path = "recovery/toolbar_count.rs"]
mod toolbar_count;

#[cfg(target_os = "macos")]
#[path = "recovery/modal_compact.rs"]
mod recovery_modal_compact;

#[cfg(target_os = "macos")]
#[path = "recovery/plan_card_994.rs"]
mod recovery_plan_card_994;

#[cfg(target_os = "macos")]
#[path = "recovery/plan_preview_files.rs"]
mod recovery_plan_preview_files;

#[cfg(target_os = "macos")]
#[path = "recovery/revert_en_summary.rs"]
mod recovery_revert_en_summary;

#[cfg(target_os = "macos")]
#[path = "recovery/oplog_keyboard.rs"]
mod recovery_oplog_keyboard;
#[cfg(target_os = "macos")]
#[path = "recovery/oplog_layout.rs"]
mod recovery_oplog_layout;
#[cfg(target_os = "macos")]
#[path = "recovery/oplog_panel.rs"]
mod recovery_oplog_panel;
#[cfg(target_os = "macos")]
#[path = "recovery/oplog_restore_keys.rs"]
mod recovery_oplog_restore_keys;
#[cfg(target_os = "macos")]
#[path = "recovery/oplog_select.rs"]
mod recovery_oplog_select;
#[cfg(target_os = "macos")]
#[path = "recovery/worktree_lock_reason.rs"]
mod recovery_worktree_lock_reason;

#[cfg(target_os = "macos")]
#[path = "recovery/worktree_ports.rs"]
mod recovery_worktree_ports;

#[cfg(target_os = "macos")]
#[path = "recovery/worktree_remove_shell.rs"]
mod recovery_worktree_remove_shell;

#[cfg(target_os = "macos")]
#[path = "recovery/worktree_inspection.rs"]
mod worktree_inspection;

#[cfg(target_os = "macos")]
#[path = "recovery/read_owner.rs"]
mod read_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/tab_ui_state.rs"]
mod tab_ui_state;

#[cfg(target_os = "macos")]
#[path = "recovery/pane_resources.rs"]
mod pane_resources;

#[cfg(target_os = "macos")]
#[path = "recovery/host_dirty_guard.rs"]
mod host_dirty_guard;

#[cfg(target_os = "macos")]
#[path = "recovery/host_guard_hold_reason.rs"]
mod host_guard_hold_reason;

#[cfg(target_os = "macos")]
#[path = "recovery/host_guard_async_restart.rs"]
mod host_guard_async_restart;

#[cfg(target_os = "macos")]
#[path = "recovery/cache_history_owner.rs"]
mod cache_history_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/remote_refresh_owner.rs"]
mod remote_refresh_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/fetch_owner.rs"]
mod fetch_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/fetch_auth_error.rs"]
mod fetch_auth_error;

#[cfg(target_os = "macos")]
#[path = "recovery/guard_writer_panic.rs"]
mod guard_writer_panic;

#[cfg(target_os = "macos")]
#[path = "recovery/file_menu_owner.rs"]
mod file_menu_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/issue_write_owner.rs"]
mod issue_write_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/ghe_viewer_login.rs"]
mod ghe_viewer_login;

#[cfg(target_os = "macos")]
#[path = "recovery/platform_menu_scroll.rs"]
mod platform_menu_scroll;

#[cfg(target_os = "macos")]
#[path = "recovery/platform_menu_focus.rs"]
mod platform_menu_focus;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_menu_bounds.rs"]
mod pr_menu_bounds;

#[cfg(target_os = "macos")]
#[path = "recovery/field_picker_owner.rs"]
mod field_picker_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/issue_create_fields.rs"]
mod issue_create_fields;

#[cfg(target_os = "macos")]
#[path = "recovery/evidence_support.rs"]
mod evidence_support;

#[cfg(target_os = "macos")]
#[path = "recovery/github_evidence_owner.rs"]
mod github_evidence_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_peek_visible.rs"]
mod pr_peek_visible;

#[cfg(target_os = "macos")]
#[path = "recovery/cleanup_evidence_owner.rs"]
mod cleanup_evidence_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/conflict_evidence_owner.rs"]
mod conflict_evidence_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/cleanup_publish_owner.rs"]
mod cleanup_publish_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/ecosystem_evidence_owner.rs"]
mod ecosystem_evidence_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/squash_evidence_owner.rs"]
mod squash_evidence_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/squash_publish_owner.rs"]
mod squash_publish_owner;

#[cfg(target_os = "macos")]
#[path = "recovery/squash_graph_routing.rs"]
mod squash_graph_routing;

#[cfg(target_os = "macos")]
#[path = "recovery/worktree_graph.rs"]
mod worktree_graph;

#[cfg(target_os = "macos")]
#[path = "recovery/folder_drop.rs"]
mod folder_drop;

#[cfg(target_os = "macos")]
#[path = "recovery/folder_urls.rs"]
mod folder_urls;

#[cfg(target_os = "macos")]
#[path = "recovery/workspace_mode_toolbar.rs"]
mod workspace_mode_toolbar;

#[cfg(target_os = "macos")]
#[path = "recovery/issue_conversation_selection.rs"]
mod issue_conversation_selection;

#[cfg(target_os = "macos")]
#[path = "recovery/issue_conversation_lifecycle.rs"]
mod issue_conversation_lifecycle;

#[cfg(target_os = "macos")]
#[path = "recovery/issue_enabled_input_copy.rs"]
mod issue_enabled_input_copy;

#[cfg(target_os = "macos")]
#[path = "recovery/issue_offscreen_select_all.rs"]
mod issue_offscreen_select_all;

#[cfg(target_os = "macos")]
#[path = "recovery/toolbar_keyboard.rs"]
mod toolbar_keyboard;

#[cfg(target_os = "macos")]
#[path = "recovery/issues_pagination.rs"]
mod issues_pagination;

#[cfg(target_os = "macos")]
#[path = "recovery/pr_pagination.rs"]
mod pr_pagination;

#[cfg(target_os = "macos")]
#[path = "perf/oplog_detail.rs"]
mod perf_oplog_detail;

#[cfg(target_os = "macos")]
#[path = "recovery/inspector_metadata.rs"]
mod inspector_metadata;

#[cfg(target_os = "macos")]
#[path = "recovery/inspector_message_links.rs"]
mod inspector_message_links;

#[cfg(target_os = "macos")]
#[path = "recovery/finder_scale_editor.rs"]
mod finder_scale_editor;

#[cfg(target_os = "macos")]
#[path = "recovery/finder_scale_history.rs"]
mod finder_scale_history;

#[cfg(target_os = "macos")]
#[path = "recovery/density_rest.rs"]
mod density_rest;
#[cfg(target_os = "macos")]
#[path = "recovery/finder_scale_rest.rs"]
mod finder_scale_rest;

#[cfg(target_os = "macos")]
#[path = "recovery/inspector_file_rows.rs"]
mod inspector_file_rows;
#[cfg(target_os = "macos")]
#[path = "recovery/inspector_scale.rs"]
mod inspector_scale;

#[cfg(target_os = "macos")]
#[path = "recovery/text_caps.rs"]
mod text_caps;

#[cfg(target_os = "macos")]
#[path = "perf/inspector_derived.rs"]
mod perf_inspector_derived;

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use crate::proc_identity;

    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{
        point, px, size, AnyWindowHandle, App, Bounds, Entity, Modifiers, MouseButton, Pixels,
        Render, Size, VisualTestAppContext, Window as GpuiWindow, WindowBounds, WindowHandle,
        WindowOptions,
    };
    use kagi::graph::{EdgeKind, GraphEdge};
    use kagi::ui::{
        commands::CreateSnapshot, commit_list, commit_panel::CommitPanelFileRef,
        diff_view::MainDiffSource, e2e, editor_tree_menu::EditorTreeAction, graph_wip, oplog_panel,
        settings::CopyTarget, theme, BottomTab, CopyDiffSelection, KagiApp, ToggleBottomPanel,
    };

    #[link(name = "objc")]
    extern "C" {
        fn objc_autoreleasePoolPush() -> *mut std::ffi::c_void;
        fn objc_autoreleasePoolPop(pool: *mut std::ffi::c_void);
        fn objc_getClass(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
        fn object_getClass(object: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }

    /// The offscreen runner has no AppKit event loop to drain native windows.
    /// MacWindow::drop queues close/autorelease; drain while GPUI is still alive
    /// so native frame callbacks release their captured InputState entities.
    struct NativeAutoreleasePool(*mut std::ffi::c_void);

    impl NativeAutoreleasePool {
        fn new() -> Self {
            Self(unsafe { objc_autoreleasePoolPush() })
        }
    }

    impl Drop for NativeAutoreleasePool {
        fn drop(&mut self) {
            // Created and drained on this runner's macOS main thread.
            unsafe { objc_autoreleasePoolPop(self.0) };
        }
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: *const std::ffi::c_void;
        fn CFRunLoopRunInMode(
            mode: *const std::ffi::c_void,
            seconds: f64,
            return_after_source: u8,
        ) -> i32;
    }

    pub(crate) fn drain_native_events() {
        // MacWindow closes on MacPlatform's native executor, not TestDispatcher.
        unsafe {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 0);
        }
    }

    /// Supply a native frame tick for one hidden test window. AppKit stops its
    /// display link while occluded, so draining the runloop alone cannot execute
    /// GPUI's on_next_frame callbacks. GPUIView::displayLayer enters the same
    /// request-frame callback with default options: it does not invalidate the
    /// window or force rendering. In particular, a prepaint refresh ignored by
    /// GPUI stays ignored; only a genuinely scheduled refresh draws another frame.
    pub(crate) fn request_native_frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
        let title = format!("Kagi native-frame test {:?}", window.window_id());
        cx.update_window(window, |_, window, _| {
            if window.window_title() != title {
                window.set_window_title(&title);
            }
        })
        .expect("native frame target still mounted");
        // Do not hold the GPUI App borrow here: displayLayer re-enters its
        // request-frame callback, which updates that same App.
        unsafe {
            type Object = *mut std::ffi::c_void;
            let send: unsafe extern "C" fn(Object, Object) -> Object =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let index: unsafe extern "C" fn(Object, Object, usize) -> Object =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let count_windows: unsafe extern "C" fn(Object, Object) -> usize =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let frame: unsafe extern "C" fn(Object, Object, Object) =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let application = send(
                objc_getClass(c"NSApplication".as_ptr()),
                sel_registerName(c"sharedApplication".as_ptr()),
            );
            assert!(!application.is_null(), "runner owns native NSApplication");
            let windows = send(application, sel_registerName(c"windows".as_ptr()));
            let count = count_windows(windows, sel_registerName(c"count".as_ptr()));
            let mut target: Object = std::ptr::null_mut();
            for position in 0..count {
                let candidate = index(
                    windows,
                    sel_registerName(c"objectAtIndex:".as_ptr()),
                    position,
                );
                let name = send(candidate, sel_registerName(c"title".as_ptr()));
                let utf8 = send(name, sel_registerName(c"UTF8String".as_ptr()));
                if !utf8.is_null()
                    && std::ffi::CStr::from_ptr(utf8.cast()).to_bytes() == title.as_bytes()
                {
                    assert!(target.is_null(), "native frame title identifies one window");
                    // Pinned MacWindow embeds GPUIView directly under AppKit's
                    // contentView, alongside any separate blurred background.
                    let content = send(candidate, sel_registerName(c"contentView".as_ptr()));
                    let children = send(content, sel_registerName(c"subviews".as_ptr()));
                    let count = count_windows(children, sel_registerName(c"count".as_ptr()));
                    for child in 0..count {
                        let view = index(
                            children,
                            sel_registerName(c"objectAtIndex:".as_ptr()),
                            child,
                        );
                        if object_getClass(view) == objc_getClass(c"GPUIView".as_ptr()) {
                            assert!(target.is_null(), "target window contains one GPUIView");
                            target = view;
                        }
                    }
                    assert!(
                        !target.is_null(),
                        "target window embeds its pinned GPUIView"
                    );
                }
            }
            assert!(
                !target.is_null(),
                "specific hidden native frame view exists"
            );
            assert_eq!(
                object_getClass(target),
                objc_getClass(c"GPUIView".as_ptr()),
                "only the pinned GPUI native view receives the frame tick"
            );
            frame(
                target,
                sel_registerName(c"displayLayer:".as_ptr()),
                std::ptr::null_mut(),
            );
        }
    }

    /// `git` with a deterministic identity + no user-config bleed-through.
    pub(super) fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "poc")
            .env("GIT_AUTHOR_EMAIL", "poc@example.com")
            .env("GIT_COMMITTER_NAME", "poc")
            .env("GIT_COMMITTER_EMAIL", "poc@example.com")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git");
        assert!(status.success(), "git {args:?} failed");
    }

    /// A throwaway repo with two commits on `main`.
    pub(super) fn build_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::write(p.join("README.md"), "# fixture\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "initial commit"]);
        std::fs::write(p.join("README.md"), "# fixture\nsecond line\n").unwrap();
        git(p, &["commit", "-q", "-am", "second commit"]);
        dir
    }

    /// A repo whose HEAD is an AI-agent commit (Claude Code, detected via the
    /// `Co-Authored-By: Claude <noreply@anthropic.com>` trailer — provenance
    /// Route 1) sitting on top of a plain human commit. Exercises issue #337's
    /// `CommitRow.provenance` classification through the real snapshot pipeline.
    fn build_agent_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::write(p.join("f.txt"), "one\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "human commit"]);
        std::fs::write(p.join("f.txt"), "two\n").unwrap();
        git(p, &["add", "."]);
        git(
            p,
            &[
                "commit",
                "-q",
                "-m",
                "agent commit\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
            ],
        );
        dir
    }

    /// Issue #472: a repo whose HEAD is buried — three commits stacked on a
    /// sibling branch put `main`'s HEAD at row 3 — with BOTH the main working
    /// tree and a linked worktree dirty, so the graph draws two WIP rows, each
    /// needing its own dashed connector down to its own HEAD.
    ///
    /// The worktree lives beside the repo, not inside it, so it does not show
    /// up as an untracked directory in the repo's own status.
    fn build_wip_connector_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("repo");
        let wt = root.path().join("wt");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("f.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "base"]);
        // Three commits on top of HEAD, on another branch: the revwalk emits
        // children before parents, so `main`'s HEAD lands at row 3.
        git(&repo, &["checkout", "-q", "-b", "ahead"]);
        for i in 0..3 {
            std::fs::write(repo.join("f.txt"), format!("ahead {i}\n")).unwrap();
            git(&repo, &["commit", "-q", "-am", &format!("ahead {i}")]);
        }
        git(&repo, &["checkout", "-q", "main"]);
        git(
            &repo,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "ahead"],
        );
        // Both working trees dirty → two WIP rows.
        std::fs::write(repo.join("dirty.txt"), "main\n").unwrap();
        std::fs::write(wt.join("dirty.txt"), "wt\n").unwrap();
        let repo = repo.canonicalize().unwrap();
        let wt = wt.canonicalize().unwrap();
        (root, repo, wt)
    }

    /// Issue #476 slice 2: `main` (dirty) plus **two** linked worktrees, both
    /// dirty and each on its own branch at its own commit — three WIP rows.
    ///
    /// Two worktrees, not one: committing from the first one's panel makes it
    /// clean, so its row leaves the list. With a second worktree BELOW it, a
    /// lane map keyed by row position hands that row its vanished neighbour's
    /// lane; keyed by target it keeps its own.
    fn build_two_worktree_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("repo");
        let wt_a = root.path().join("wt-a");
        let wt_b = root.path().join("wt-b");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("f.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "base"]);
        // Older history leaves room to scroll every WIP offscreen while its
        // HEAD remains visible (#773).
        for _ in 0..24 {
            git(&repo, &["commit", "--allow-empty", "-qm", "older history"]);
        }
        // A distinct commit per branch, so each WIP row's connector has its own
        // HEAD row to land on (a shared HEAD would still work, but distinct
        // rows make a shifted lane unambiguous).
        for b in ["wt-a", "wt-b"] {
            git(&repo, &["checkout", "-q", "-b", b]);
            std::fs::write(repo.join("f.txt"), format!("{b}\n")).unwrap();
            git(&repo, &["commit", "-q", "-am", &format!("{b} commit")]);
            git(&repo, &["checkout", "-q", "main"]);
        }
        git(&repo, &["checkout", "-qb", "intervening", "wt-a"]);
        git(
            &repo,
            &[
                "commit",
                "--allow-empty",
                "-qm",
                "other branch above WIP HEAD",
            ],
        );
        git(&repo, &["checkout", "-q", "main"]);
        git(
            &repo,
            &["worktree", "add", "-q", wt_a.to_str().unwrap(), "wt-a"],
        );
        git(
            &repo,
            &["worktree", "add", "-q", wt_b.to_str().unwrap(), "wt-b"],
        );
        // All three working trees dirty → three WIP rows.
        std::fs::write(repo.join("dirty.txt"), "main\n").unwrap();
        std::fs::write(wt_a.join("dirty.txt"), "a\n").unwrap();
        std::fs::write(wt_b.join("dirty.txt"), "b\n").unwrap();
        (
            root,
            repo.canonicalize().unwrap(),
            wt_a.canonicalize().unwrap(),
            wt_b.canonicalize().unwrap(),
        )
    }

    /// Issue #767: the three WIP-anchor cases the ordinary three-worktree
    /// fixture cannot show, in one repo — a linked worktree sitting on the
    /// **same commit** as the open repo's HEAD (one shared anchor column), a
    /// **detached** worktree on an older commit (its HEAD is an OID, so it
    /// still anchors), and an **orphan** worktree whose HEAD is unborn (no
    /// anchor at all). All four working trees are dirty → four WIP rows.
    ///
    /// Git refuses to check one branch out twice, so the shared case is a
    /// second branch pointing at the same commit — which is exactly the
    /// on-screen situation: two WIP rows, one HEAD commit.
    fn build_wip_anchor_edge_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("repo");
        let shared = root.path().join("wt-shared");
        let detached = root.path().join("wt-detached");
        let orphan = root.path().join("wt-orphan");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("f.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "base"]);
        let base = rev_parse(&repo, "HEAD");
        std::fs::write(repo.join("f.txt"), "tip\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "tip"]);
        git(&repo, &["branch", "shared", "main"]);
        git(
            &repo,
            &["worktree", "add", "-q", shared.to_str().unwrap(), "shared"],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                detached.to_str().unwrap(),
                &base,
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--orphan",
                orphan.to_str().unwrap(),
            ],
        );
        for dir in [&repo, &shared, &detached, &orphan] {
            std::fs::write(dir.join("dirty.txt"), "wip\n").unwrap();
        }
        (
            root,
            repo.canonicalize().unwrap(),
            shared.canonicalize().unwrap(),
            detached.canonicalize().unwrap(),
            orphan.canonicalize().unwrap(),
        )
    }

    /// `git rev-parse <rev>` for a working tree (#476 slice 3: `HEAD^`, to prove
    /// an amend replaced the tip instead of stacking on it).
    fn rev_parse(dir: &Path, rev: &str) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", rev])
            .output()
            .expect("rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Did `git <args>` exit 0 in `dir`? (`cat-file -e <sha>` is an existence
    /// probe, so the exit code IS the answer.)
    fn git_ok(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .expect("spawn git")
            .success()
    }

    /// `git rev-list --count HEAD` for a working tree.
    fn commit_count(dir: &Path) -> usize {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .expect("rev-list");
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    /// `git for-each-ref <pattern>` — the ref-existence probe for the snapshot
    /// scenario. Returns the raw stdout (one line per matching ref).
    pub(super) fn for_each_ref(dir: &Path, pattern: &str) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["for-each-ref", pattern])
            .output()
            .expect("for-each-ref");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// How many `NSWindow`s a scenario may keep alive at once (#549).
    ///
    /// Eight is generous: no scenario legitimately needs more than the four
    /// viewports of the layout matrix. The point is that the *next* runaway
    /// loop fails on window five, not on window 1,408.
    pub(crate) const MAX_LIVE_WINDOWS: usize = 8;

    thread_local! {
        /// Name of the scenario currently running, so the budget panic can say
        /// which loop leaked. Set by `run`, read only by `open_offscreen`.
        static CURRENT_SCENARIO: std::cell::Cell<&'static str> =
            const { std::cell::Cell::new("<startup>") };
    }

    pub(crate) fn set_current_scenario(name: &'static str) {
        CURRENT_SCENARIO.with(|cell| cell.set(name));
    }

    /// Open an off-screen scenario window **hidden**, under a live-window
    /// budget (#549).
    ///
    /// Two problems, one helper. First, gpui's own
    /// `VisualTestAppContext::open_offscreen_window` hardcodes `show: true`
    /// (`gpui/src/app/visual_test_context.rs:114`) and macOS clamps the
    /// (-10000, -10000) origin back onto a real display, so every mount stacks
    /// a black window on the developer's screen. With `show: false` AppKit
    /// never sends `orderFront:` (`gpui_macos/src/window.rs:1082`), so the
    /// window never joins the on-screen list. `KAGI_GUI_E2E_VISIBLE=1` puts
    /// them back for human triage.
    ///
    /// Second, hidden windows are still real `NSWindow`s. A full run once
    /// opened 1,408 of them at once and the WindowServer watchdog killed the
    /// user's login session, so this asserts the live count before opening.
    ///
    /// Visibility changes nothing else: under `test-support`
    /// `App::flush_effects` draws every dirty window itself
    /// (`gpui/src/app.rs:1566`) rather than waiting for the display link, so
    /// layout, hitboxes, action dispatch and the `simulate_*` helpers are
    /// unaffected.
    pub(crate) fn open_offscreen<V: Render + 'static>(
        cx: &mut VisualTestAppContext,
        window_size: Size<Pixels>,
        build_root: impl FnOnce(&mut GpuiWindow, &mut App) -> Entity<V>,
    ) -> WindowHandle<V> {
        let live = cx.update(|app| app.windows().len());
        assert!(
            live < MAX_LIVE_WINDOWS,
            "[gui-e2e] window budget exceeded in scenario {}: {live} live NSWindows, \
             limit {MAX_LIVE_WINDOWS} (#549). Reuse one window per viewport and mutate \
             its root entity instead of mounting a fresh one per cell, and `unmount` \
             every window you open.",
            CURRENT_SCENARIO.with(|cell| cell.get()),
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: if std::env::var_os("KAGI_GUI_E2E_ONSCREEN").is_some() {
                    point(px(80.0), px(80.0))
                } else {
                    point(px(-10000.0), px(-10000.0))
                },
                size: window_size,
            })),
            focus: false,
            show: std::env::var_os("KAGI_GUI_E2E_VISIBLE").is_some(),
            ..Default::default()
        };
        if let Some(WindowBounds::Windowed(bounds)) = &options.window_bounds {
            eprintln!(
                "[gui-e2e] window options: origin={},{} visible={}",
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                options.show
            );
        }
        cx.update(|app| app.open_window(options, build_root))
            .expect("open hidden offscreen window")
    }

    /// Mount the real `KagiApp` offscreen against `repo_path`, settle the first
    /// frame, and hand back the captured entity + window handle. Mirrors the
    /// PoC mount (ADR-0166) so every scenario builds the root identically.
    pub(super) fn mount(
        cx: &mut VisualTestAppContext,
        repo_path: &Path,
    ) -> (Entity<KagiApp>, AnyWindowHandle) {
        mount_state(cx, e2e::app_state(repo_path).expect("build app_state"))
    }

    /// Mount an already-built `KagiApp`. The Welcome / no-session startup
    /// (`KagiApp::with_error`, `main.rs:170`) has no repository to mount from,
    /// and every scenario before #722 started from an open repo — which is why
    /// CI never rendered a frame with no owning session.
    pub(super) fn mount_state(
        cx: &mut VisualTestAppContext,
        app_state: KagiApp,
    ) -> (Entity<KagiApp>, AnyWindowHandle) {
        if let Some(repo) = &app_state.repo_path {
            crate::gui_evidence::fixture(repo);
        }
        let cell: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::new(RefCell::new(None));
        let build_cell = cell.clone();
        let window = open_offscreen(cx, size(px(1440.0), px(900.0)), move |window, cx| {
            e2e::mount_root(app_state, window, cx, &build_cell)
        });
        let kagi = cell.borrow().clone().expect("kagi entity captured");
        cx.run_until_parked();
        (kagi, window.into())
    }

    /// Release an offscreen root before the next scenario mounts a native
    /// window. The root entity can retain input state and tasks after its
    /// window closes, so drop it before parking the dispatcher.
    pub(super) fn unmount<T: 'static>(
        cx: &mut VisualTestAppContext,
        entity: Entity<T>,
        window: AnyWindowHandle,
    ) {
        cx.update_window(window, |_, window, _| window.remove_window())
            .expect("close offscreen window");
        drop(entity);
        cx.update(|_| {});
        cx.run_until_parked();
        // Dropping the gpui `Window` does NOT close the `NSWindow`:
        // `impl Drop for MacWindow` (`gpui_macos/src/window.rs:1169-1192`)
        // *spawns* `window.close()` on the platform foreground executor and
        // detaches it. `VisualTestPlatform` delegates `open_window` straight to
        // the real `MacPlatform` (`gpui/src/platform/visual_test.rs:122-128`),
        // so that executor is `MacDispatcher` → `DispatchQueue::main()`
        // (`gpui_macos/src/dispatcher.rs:55-60`), not the `TestDispatcher` that
        // `run_until_parked` drains. Pump the main run loop or the window stays
        // alive — which is how a full run piled up 1,408 of them (#549).
        drain_native_events();
    }

    /// `git rev-parse HEAD` + porcelain status, for the no-mutation assertion.
    pub(super) fn repo_fingerprint(dir: &Path) -> (String, String) {
        let head = Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("rev-parse");
        let status = Command::new("git")
            .current_dir(dir)
            .args(["status", "--porcelain"])
            .output()
            .expect("status");
        (
            String::from_utf8_lossy(&head.stdout).trim().to_string(),
            String::from_utf8_lossy(&status.stdout).to_string(),
        )
    }

    fn out_dir() -> PathBuf {
        let base = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
        let dir = PathBuf::from(base).join("gui_e2e_poc");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Parse the optional comma-separated scenario substring filter. Empty
    /// entries are ignored; only an unset variable runs the full suite.
    fn scenario_filters() -> Option<Vec<String>> {
        std::env::var_os("KAGI_GUI_E2E_ONLY").map(|value| {
            value
                .to_string_lossy()
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(str::to_owned)
                .collect()
        })
    }

    /// The scenario suite. Returns a process exit code (0 = pass). Each scenario
    /// asserts observable `KagiApp` state / clipboard / repo refs (ADR-0166 §3:
    /// deterministic assertions are the oracle, screenshots are triage only) and
    /// prints a `[gui-e2e] PASS …` line. A failed assertion panics → exit 101.
    pub fn run() -> i32 {
        use crate::sidebar_identity::{
            scenario_remote_group_header_fit_cell as group_cell,
            scenario_tag_unloaded_target_cell as tag_cell,
        };
        use kagi_ui_core::i18n::Lang::{En, Ja};
        // Opt-in: real Metal + a main-thread window is an *evidence lane*, not a
        // required gate (ADR-0166 §CI). Unset → skip so `cargo test --workspace`
        // stays fast and non-flaky. Set `KAGI_GUI_E2E=1` to run it.
        if std::env::var_os("KAGI_GUI_E2E").is_none() {
            eprintln!("[gui-e2e] SKIP: set KAGI_GUI_E2E=1 to run the visual scenarios");
            return 0;
        }
        let filters = scenario_filters();
        let exact = std::env::var("KAGI_GUI_E2E_EXACT").ok();
        let mut scenarios: Vec<Scenario> = vec![
            (
                "stash_drop_persists",
                Box::new(crate::recovery_operations::scenario_stash_drop_persists),
            ),
            (
                "history_persists",
                Box::new(crate::recovery_operations::scenario_history_persists),
            ),
            (
                "cleanup_stale_tab",
                Box::new(crate::recovery_operations::scenario_cleanup_stale_tab),
            ),
            (
                "modal_sections",
                Box::new(crate::recovery_layout::scenario_modal_sections),
            ),
            (
                "modal_compact",
                Box::new(crate::recovery_modal_compact::scenario_modal_compact),
            ),
            (
                "worktree_lock_reason",
                Box::new(crate::recovery_worktree_lock_reason::scenario_worktree_lock_reason),
            ),
            (
                "unborn_empty_state",
                Box::new(crate::recovery_unborn_empty::scenario_unborn_empty_state),
            ),
            ("terminal_paste_bracketed", Box::new(crate::recovery_terminal_keys::scenario_terminal_paste_bracketed)),
            ("terminal_paste_control", Box::new(crate::recovery_terminal_keys::scenario_terminal_paste_control)),
            ("terminal_focus_exit", Box::new(crate::recovery_terminal_keys::scenario_terminal_focus_exit)),
            ("terminal_hide_focus", Box::new(crate::recovery_terminal_keys::scenario_terminal_hide_focus)),
            (
                "terminal_states",
                Box::new(crate::recovery_terminal_states::scenario_terminal_states),
            ),
            (
                "terminal_font_fallback",
                Box::new(crate::recovery_terminal_fonts::scenario_terminal_font_fallback),
            ),
            (
                "terminal_auto_lock",
                Box::new(crate::recovery_worktree_lock_reason::scenario_terminal_auto_lock),
            ),
            (
                "terminal_auto_lock_race",
                Box::new(crate::recovery_worktree_lock_reason::scenario_terminal_auto_lock_race),
            ),
            (
                "worktree_port_env",
                Box::new(crate::recovery_worktree_ports::scenario_worktree_port_env),
            ),
            (
                "worktree_nonconcurrent",
                Box::new(crate::recovery_worktree_ports::scenario_worktree_nonconcurrent),
            ),
            (
                "worktree_remove_live_shell",
                Box::new(crate::recovery_worktree_remove_shell::scenario_worktree_remove_live_shell),
            ),
            (
                "external_lock_reload",
                Box::new(crate::recovery_worktree_lock_reason::scenario_external_lock_reload),
            ),
            (
                "worktree_prune_repair_receipt",
                Box::new(crate::recovery_worktree_lock_reason::scenario_worktree_prune_repair_receipt),
            ),
            (
                "worktree_admin_result_owner",
                Box::new(crate::recovery_worktree_lock_reason::scenario_worktree_admin_result_owner),
            ),
            (
                "preflight_presentation",
                Box::new(crate::recovery_operations::scenario_preflight_presentation),
            ),
            (
                "unmerged_branch_delete_armed",
                Box::new(crate::recovery_operations::scenario_unmerged_branch_delete_armed),
            ),
            (
                "modal_no_fallthrough",
                Box::new(crate::recovery_operations::scenario_modal_no_fallthrough),
            ),
            (
                "blocked_plan_command",
                Box::new(crate::recovery_operations::scenario_blocked_plan_command),
            ),
            (
                "plan_heading_chipless",
                Box::new(crate::recovery_plan_card_994::scenario_plan_heading_chipless),
            ),
            (
                "plan_preview_files",
                Box::new(crate::recovery_plan_preview_files::scenario_plan_preview_files),
            ),
            (
                "revert_card_en_summary",
                Box::new(crate::recovery_revert_en_summary::scenario_revert_card_en_summary),
            ),
            (
                "plan_preview_files_empty",
                Box::new(crate::recovery_plan_preview_files::scenario_plan_preview_files_empty),
            ),
            (
                "plan_recovery_noop",
                Box::new(crate::recovery_plan_card_994::scenario_plan_recovery_noop),
            ),
            (
                "plan_card_heading",
                Box::new(crate::recovery_plan_card_994::scenario_plan_card_heading),
            ),
            (
                "bespoke_plan_heading",
                Box::new(crate::recovery_plan_card_994::scenario_bespoke_plan_heading),
            ),
            (
                "bespoke_recovery_1023",
                Box::new(crate::recovery_plan_card_994::scenario_bespoke_recovery_1023),
            ),
            (
                "plan_recovery_commands",
                Box::new(crate::recovery_plan_card_994::scenario_plan_recovery_commands),
            ),
            (
                "plan_recovery_ax",
                Box::new(crate::recovery_plan_card_994::scenario_plan_recovery_ax),
            ),
            (
                "plan_equivalent_summary",
                Box::new(crate::recovery_plan_card_994::scenario_plan_equivalent_summary),
            ),
            (
                "commit_list_roles",
                Box::new(crate::recovery_operations::scenario_commit_list_roles),
            ),
            (
                "sidebar_tree_roles",
                Box::new(crate::recovery_operations::scenario_sidebar_tree_roles),
            ),
            (
                "file_tree_roles",
                Box::new(crate::file_tree_a11y::scenario_file_tree_roles),
            ),
            (
                "sidebar_panes",
                Box::new(crate::recovery_sidebar_panes::scenario_sidebar_panes),
            ),
            (
                "sidebar_tree_pane_motion",
                Box::new(crate::recovery_sidebar_tree_motion::scenario_sidebar_tree_pane_motion),
            ),
            (
                "sidebar_tree_subgroup_motion",
                Box::new(crate::recovery_sidebar_tree_motion::scenario_sidebar_tree_subgroup_motion),
            ),
            (
                "sidebar_tree_pointer_motion",
                Box::new(crate::recovery_sidebar_tree_motion::scenario_sidebar_tree_pointer_motion),
            ),
            (
                "sidebar_tree_selection_motion",
                Box::new(crate::recovery_sidebar_tree_motion::scenario_sidebar_tree_selection_motion),
            ),
            (
                "sidebar_tree_focus_return_motion",
                Box::new(
                    crate::recovery_sidebar_tree_motion::scenario_sidebar_tree_focus_return_motion,
                ),
            ),
            (
                "sidebar_tree_compact_focus_return_motion",
                Box::new(
                    crate::recovery_sidebar_tree_motion::scenario_sidebar_tree_compact_focus_return_motion,
                ),
            ),
            (
                "sidebar_tree_refresh_motion",
                Box::new(crate::recovery_sidebar_tree_motion::scenario_sidebar_tree_refresh_motion),
            ),
            (
                "editor_directory_motion",
                Box::new(crate::recovery_editor_tree_motion::scenario_editor_directory_motion),
            ),
            (
                "editor_tree_pointer_motion",
                Box::new(crate::recovery_editor_tree_motion::scenario_editor_tree_pointer_motion),
            ),
            (
                "sidebar_search_escape",
                Box::new(crate::sidebar_search::sidebar_search_escape),
            ),
            (
                "sidebar_search_clear_ax",
                Box::new(crate::sidebar_search::sidebar_search_clear_ax),
            ),
            (
                "sidebar_filter_tab_entry",
                Box::new(crate::sidebar_filter::scenario_sidebar_filter_tab_entry),
            ),
            (
                "input_names_inventory",
                Box::new(crate::input_names::scenario_input_names_inventory),
            ),
            (
                "input_ime_escape_boundaries",
                Box::new(crate::input_names::scenario_input_ime_escape_boundaries),
            ),
            (
                "branch_rename_neutral_state",
                Box::new(crate::rename_form_state::scenario_branch_rename_neutral_state),
            ),
            (
                "hairline_dividers",
                Box::new(crate::recovery_hairline_dividers::scenario_hairline_dividers),
            ),
            (
                "pr_list_roles",
                Box::new(crate::recovery_operations::scenario_pr_list_roles),
            ),
            (
                "pr_dashboard_keyboard",
                Box::new(crate::pr_dashboard_keyboard::scenario_pr_dashboard_keyboard),
            ),
            (
                "pr_dashboard_hover_selected",
                Box::new(crate::pr_dashboard_hover_selected::scenario_pr_dashboard_hover_selected),
            ),
            (
                "pr_filter_strip_keyboard",
                Box::new(crate::pr_dashboard_keyboard::scenario_pr_filter_strip_keyboard),
            ),
            (
                "relative_time_ja",
                Box::new(crate::time_locale::scenario_relative_time_ja),
            ),
            (
                "cleanup_local_date",
                Box::new(crate::time_locale::scenario_cleanup_local_date),
            ),
            (
                "pr_dashboard_responsive",
                Box::new(crate::recovery_layout::scenario_pr_dashboard_responsive),
            ),
            (
                "pr_dashboard_draft_contrast_light_narrow",
                Box::new(|cx| crate::pr_dashboard_contrast::scenario_pr_dashboard_draft_contrast(cx, (940., 660.), "apple-light")),
            ),
            (
                "pr_dashboard_draft_contrast_dark_narrow",
                Box::new(|cx| crate::pr_dashboard_contrast::scenario_pr_dashboard_draft_contrast(cx, (940., 660.), "apple-dark")),
            ),
            (
                "pr_dashboard_draft_contrast_light_wide",
                Box::new(|cx| crate::pr_dashboard_contrast::scenario_pr_dashboard_draft_contrast(cx, (1392., 883.), "apple-light")),
            ),
            (
                "pr_dashboard_draft_contrast_dark_wide",
                Box::new(|cx| crate::pr_dashboard_contrast::scenario_pr_dashboard_draft_contrast(cx, (1392., 883.), "apple-dark")),
            ),
            (
                "color_vision_theme",
                Box::new(crate::recovery_operations::scenario_color_vision_theme),
            ),
            (
                "theme_custom",
                Box::new(crate::theme_custom::scenario_theme_custom),
            ),
            (
                "theme_folder_controls",
                Box::new(crate::theme_custom::scenario_theme_folder_controls),
            ),
            (
                "wip_selected_roles",
                Box::new(crate::recovery_operations::scenario_wip_selected_roles),
            ),
            (
                "primary_button_states",
                Box::new(crate::primary_button::scenario_primary_button_states),
            ),
            (
                "modal_count_contrast",
                Box::new(crate::modal_count_contrast::scenario_modal_count_contrast),
            ),
            (
                "modal_scale_targets",
                Box::new(crate::modal_scale::scenario_modal_scale_targets),
            ),
            (
                "modal_scale_notes",
                Box::new(crate::modal_scale::scenario_modal_scale_notes),
            ),
            (
                "modal_scale_note_paths",
                Box::new(crate::modal_note_paths::scenario_modal_scale_note_paths),
            ),
            (
                "dark_elevation_palette",
                Box::new(crate::dark_elevation::scenario_dark_elevation_palette),
            ),
            (
                "dark_elevation_modal",
                Box::new(crate::dark_elevation::scenario_dark_elevation_modal),
            ),
            (
                "dark_elevation_menu",
                Box::new(crate::dark_elevation::scenario_dark_elevation_menu),
            ),
            (
                "dark_elevation_popover",
                Box::new(crate::dark_elevation::scenario_dark_elevation_popover),
            ),
            (
                "dark_elevation_toast",
                Box::new(crate::dark_elevation::scenario_dark_elevation_toast),
            ),
            (
                "dark_elevation_chrome",
                Box::new(crate::dark_elevation::scenario_dark_elevation_chrome),
            ),
            (
                "system_accent_chrome",
                Box::new(crate::system_accent::scenario_system_accent_chrome),
            ),
            (
                "nagi_fixed_accent",
                Box::new(crate::system_accent::scenario_nagi_fixed_accent),
            ),
            (
                "system_reduce_motion",
                Box::new(crate::system_accessibility::scenario_system_reduce_motion),
            ),
            (
                "system_reduce_transparency",
                Box::new(crate::system_accessibility::scenario_system_reduce_transparency),
            ),
            ("menus_localized", Box::new(crate::menus_ja::scenario_menus_localized)),
            ("edit_menu_undo_redo", Box::new(crate::menus_ja::scenario_edit_menu_undo_redo)),
            ("file_menu_save", Box::new(crate::menus_ja::scenario_file_menu_save)),
            (
                "window_title_identity",
                Box::new(crate::menus_window::scenario_window_title_identity),
            ),
            (
                "view_menu_state_labels",
                Box::new(crate::menus_window::scenario_view_menu_state_labels),
            ),
            (
                "conflict_repository_menu",
                Box::new(crate::menus_window::scenario_conflict_repository_menu),
            ),
            (
                "window_menu_items",
                Box::new(crate::menus_window::scenario_window_menu_items),
            ),
            (
                "window_menu_front_order",
                Box::new(crate::menus_window::scenario_window_menu_front_order),
            ),
            (
                "about_dialog",
                Box::new(crate::menus_window::scenario_about_dialog),
            ),
            (
                "tab_restore_active_identity",
                Box::new(crate::tab_restore_active_identity::scenario_tab_restore_active_identity),
            ),
            (
                "close_window_initial_home",
                Box::new(crate::close_window_initial_home::scenario_close_window_initial_home),
            ),
            (
                "shortcuts_panel_scroll",
                Box::new(crate::shortcuts_panel_scroll::scenario_shortcuts_panel_scroll),
            ),
            (
                "help_shortcuts_glyphs",
                Box::new(crate::help_shortcuts_glyphs::scenario_help_shortcuts_glyphs),
            ),
            (
                "window_restore_position",
                Box::new(crate::window_restore::scenario_window_restore_position),
            ),
            (
                "window_min_size",
                Box::new(crate::window_min_size::scenario_window_min_size),
            ),
            (
                "window_restore_display_identity",
                Box::new(crate::window_restore::scenario_window_restore_display_identity),
            ),
            (
                "window_restore_fit",
                Box::new(crate::window_restore::scenario_window_restore_fit),
            ),
            (
                "window_restore_titlebar",
                Box::new(crate::window_restore::scenario_window_restore_titlebar),
            ),
            (
                "settings_window_composition",
                Box::new(crate::settings_window::scenario_settings_window_composition),
            ),
            (
                "settings_window_screen_fit",
                Box::new(crate::settings_window::scenario_settings_window_screen_fit),
            ),
            (
                "settings_window_open_raise",
                Box::new(crate::settings_window::scenario_settings_window_open_raise),
            ),
            (
                "settings_window_close_keeps_repo",
                Box::new(crate::settings_window::scenario_settings_window_close_keeps_repo),
            ),
            (
                "settings_window_live_apply",
                Box::new(crate::settings_window::scenario_settings_window_live_apply),
            ),
            (
                "settings_window_owner_release",
                Box::new(crate::settings_window::scenario_settings_window_owner_release),
            ),
            (
                "settings_window_position_restore",
                Box::new(crate::settings_window::scenario_settings_window_position_restore),
            ),
            (
                "settings_window_dock_reopen",
                Box::new(crate::settings_window::scenario_settings_window_dock_reopen),
            ),
            (
                "settings_window_host_isolation",
                Box::new(crate::settings_window::scenario_settings_window_host_isolation),
            ),
            (
                "settings_quiet",
                Box::new(crate::settings_quiet::scenario_settings_quiet),
            ),
            (
                "settings_radio_dot",
                Box::new(crate::settings_quiet::scenario_settings_radio_dot),
            ),
            (
                "inactive_selection",
                Box::new(crate::inactive_selection::scenario_inactive_selection),
            ),
            (
                "selection_on_accent",
                Box::new(crate::selection_on_accent::scenario_selection_on_accent),
            ),
            (
                "graph_selection_wash",
                Box::new(crate::graph_selection_wash::scenario_graph_selection_wash),
            ),
            (
                "graph_selection_crossfade",
                Box::new(crate::graph_selection_wash::scenario_graph_selection_crossfade),
            ),
            (
                "graph_mode_motion_reset",
                Box::new(crate::graph_motion_edges::scenario_graph_mode_motion_reset),
            ),
            (
                "graph_commit_insert_motion",
                Box::new(crate::graph_selection_wash::scenario_graph_commit_insert_motion),
            ),
            (
                "graph_wip_insert_motion",
                Box::new(crate::graph_selection_wash::scenario_graph_wip_insert_motion),
            ),
            (
                "graph_target_crossfade",
                Box::new(crate::graph_selection_wash::scenario_graph_target_crossfade),
            ),
            (
                "graph_unborn_wip_motion",
                Box::new(crate::graph_motion_edges::scenario_graph_unborn_wip_motion),
            ),
            (
                "graph_activation_motion_reset",
                Box::new(crate::graph_motion_edges::scenario_graph_activation_motion_reset),
            ),
            (
                "graph_insert_motion_hit_targets",
                Box::new(crate::graph_motion_edges::scenario_graph_insert_motion_hit_targets),
            ),
            (
                "graph_over_row_folder_drop",
                Box::new(crate::graph_motion_edges::scenario_graph_over_row_folder_drop),
            ),
            (
                "graph_stash_pointer_focus",
                Box::new(crate::graph_motion_edges::scenario_graph_stash_pointer_focus),
            ),
            (
                "graph_wip_hover_wash",
                Box::new(crate::graph_selection_wash::scenario_graph_wip_hover_wash),
            ),
            (
                "repo_tabs_neutral",
                Box::new(crate::inactive_selection::scenario_repo_tabs_neutral),
            ),
            (
                "tab_strip_overflow",
                Box::new(crate::tab_strip::scenario_tab_strip_overflow),
            ),
            (
                "tab_close_names",
                Box::new(crate::tab_strip::scenario_tab_close_names),
            ),
            (
                "titlebar_tabs",
                Box::new(crate::titlebar_tabs::scenario_titlebar_tabs),
            ),
            (
                "titlebar_tabs_resize",
                Box::new(crate::titlebar_tabs::scenario_titlebar_tabs_resize),
            ),
            (
                "dialog_a11y_roles",
                Box::new(crate::recovery_operations::scenario_dialog_a11y_roles),
            ),
            (
                "input_code_context_menus",
                Box::new(crate::input_context_menus::scenario_input_code_context_menus),
            ),
            (
                "input_context_menu_locale",
                Box::new(crate::destructive_modal_return::scenario_input_context_menu_locale),
            ),
            (
                "command_period_overflow",
                Box::new(crate::input_context_menus::scenario_command_period_overflow),
            ),
            (
                "command_period_settings_select",
                Box::new(crate::input_context_menus::scenario_command_period_settings_select),
            ),
            (
                "compact_input_escape",
                Box::new(crate::input_context_menus::scenario_compact_input_escape),
            ),
            (
                "compact_input_command_period",
                Box::new(crate::input_context_menus::scenario_compact_input_command_period),
            ),
            (
                "modal_command_period",
                Box::new(crate::destructive_modal_return::scenario_modal_command_period),
            ),
            (
                "destructive_modal_return",
                Box::new(crate::destructive_modal_return::scenario_destructive_modal_return),
            ),
            (
                "dirty_guard_focus_trap",
                Box::new(crate::destructive_modal_return::scenario_dirty_guard_focus_trap),
            ),
            (
                "alert_focus_reload",
                Box::new(crate::destructive_modal_return::scenario_alert_focus_reload),
            ),
            (
                "alert_focus_notice",
                Box::new(crate::destructive_modal_return::scenario_alert_focus_notice),
            ),
            (
                "sync_to_remote_armed",
                Box::new(crate::recovery_operations::scenario_sync_to_remote_armed),
            ),
            (
                "sync_footer_cancel_visible",
                Box::new(crate::sync_layout::scenario_sync_footer_cancel_visible),
            ),
            ("rebase_plan_labels", Box::new(crate::plan_labels::scenario_rebase_plan_labels)),
            (
                "rebase_card_scope",
                Box::new(crate::rebase_card_scope::scenario_rebase_card_scope),
            ),
            (
                "push_tag_card_scope",
                Box::new(crate::push_tag_card_scope::scenario_push_tag_card_scope),
            ),
            (
                "neighbor_plan_labels",
                Box::new(crate::plan_card_reachability::scenario_neighbor_plan_labels),
            ),
            ("plan_footer_long_branch", Box::new(crate::plan_labels::scenario_plan_footer_long_branch)),
            ("push_plan_subjects", Box::new(crate::plan_labels::scenario_push_plan_subjects)),
            (
                "replay_onto_armed",
                Box::new(crate::recovery_operations::scenario_replay_onto_armed),
            ),
            (
                "remote_browse_modal_routing",
                Box::new(crate::recovery_operations::scenario_remote_browse_modal_routing),
            ),
            (
                "graph_escape_clears_selection",
                Box::new(crate::recovery_operations::scenario_graph_escape_clears_selection),
            ),
            (
                "window_modal_exclusivity",
                Box::new(crate::recovery_operations::scenario_window_modal_exclusivity),
            ),
            (
                "app_notice_modal_replacement",
                Box::new(crate::recovery_operations::scenario_app_notice_modal_replacement),
            ),
            (
                "app_notice_escape_dismiss",
                Box::new(crate::reconcile_unobservable::scenario_app_notice_escape_dismiss),
            ),
            (
                "app_notice_acknowledge_dismiss",
                Box::new(crate::reconcile_unobservable::scenario_app_notice_acknowledge_dismiss),
            ),
            (
                "app_notice_queue_recurrence",
                Box::new(crate::op_queue::scenario_app_notice_queue_recurrence),
            ),
            (
                "reconcile_unobservable_release",
                Box::new(crate::reconcile_unobservable::scenario_reconcile_unobservable_release),
            ),
            (
                "home_recent_remove",
                Box::new(crate::home_recent_remove::scenario_home_recent_remove),
            ),
            (
                "home_repository_affordances",
                Box::new(crate::home_rows::scenario_home_repository_affordances),
            ),
            (
                "home_tab",
                Box::new(crate::home_tab::scenario_home_tab),
            ),
            (
                "home_heading",
                Box::new(crate::home_heading::scenario_home_heading),
            ),
            (
                "home_github",
                Box::new(crate::home_github::scenario_home_github),
            ),
            (
                "home_empty_section_wording",
                Box::new(crate::home_empty_section_wording::scenario_home_empty_section_wording),
            ),
            (
                "home_work",
                Box::new(crate::home_work::scenario_home_work),
            ),
            (
                "keyboard_nav",
                Box::new(crate::keyboard_nav::scenario_keyboard_nav),
            ),
            (
                "graph_tab_stop",
                Box::new(crate::graph_tab_stop::scenario_graph_tab_stop),
            ),
            (
                "home_rows",
                Box::new(crate::home_rows::scenario_home_rows),
            ),
            (
                "issues_rows_keyboard",
                Box::new(crate::lists_keyboard::scenario_issues_rows_keyboard),
            ),
            (
                "home_recent_rows_keyboard",
                Box::new(crate::lists_keyboard::scenario_home_recent_rows_keyboard),
            ),
            (
                "branch_picker_keyboard",
                Box::new(crate::lists_keyboard::scenario_branch_picker_keyboard),
            ),
            (
                "home_row_paging",
                Box::new(crate::home_rows::scenario_home_row_paging),
            ),
            (
                "commit_paging",
                Box::new(crate::commit_paging::scenario_commit_paging),
            ),
            (
                "branch_menu_compact",
                Box::new(crate::branch_menu_compact::scenario_branch_menu_compact),
            ),
            (
                "branch_menu_merge_labels",
                Box::new(crate::state_labels::branch_menu_merge_labels),
            ),
            (
                "branch_cleanup_target_heading",
                Box::new(crate::state_labels::branch_cleanup_target_heading),
            ),
            (
                "context_menu_keys",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys),
            ),
            (
                "context_menu_keys_sidebar",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_sidebar),
            ),
            (
                "context_menu_keys_home",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_home),
            ),
            (
                "context_menu_keys_short",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_short),
            ),
            (
                "context_menu_keys_reload",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_reload),
            ),
            (
                "context_menu_keys_tab",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_tab),
            ),
            (
                "context_menu_keys_covered",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_covered),
            ),
            (
                "context_menu_keys_a11y",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_a11y),
            ),
            (
                "context_menu_keys_covered_row",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_covered_row),
            ),
            (
                "context_menu_keys_disabled_live",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_disabled_live),
            ),
            (
                "context_menu_keys_item_appears",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_item_appears),
            ),
            (
                "context_menu_keys_row_behind_info",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_row_behind_info),
            ),
            (
                "context_menu_keys_last_tab",
                Box::new(crate::context_menu_keys::scenario_context_menu_keys_last_tab),
            ),
            (
                "pr_menu_bounds",
                Box::new(crate::pr_menu_bounds::scenario_pr_menu_bounds),
            ),
            (
                "home_list_place",
                Box::new(crate::home_list_place::scenario_home_list_place),
            ),
            (
                "home_search_incomplete_repos",
                Box::new(crate::home_search_incomplete::scenario_home_search_incomplete_repos),
            ),
            (
                "home_search_incomplete_refreshing",
                Box::new(crate::home_search_incomplete::scenario_home_search_incomplete_refreshing),
            ),
            (
                "home_search_incomplete_work",
                Box::new(crate::home_search_incomplete::scenario_home_search_incomplete_work),
            ),
            (
                "home_review_avatar_host",
                Box::new(crate::home_p2::scenario_home_review_avatar_host),
            ),
            (
                "clone_card_ticker",
                Box::new(crate::home_p2::scenario_clone_card_ticker),
            ),
            (
                "clone_card_viewport",
                Box::new(crate::clone_card::scenario_clone_card_viewport),
            ),
            (
                "clone_menu_routes_home",
                Box::new(crate::clone_menu::scenario_clone_menu_routes_home),
            ),
            (
                "clone_card_destination",
                Box::new(crate::clone_card::scenario_clone_card_destination),
            ),
            (
                "clone_picker_owner",
                Box::new(crate::clone_owner::scenario_clone_picker_owner),
            ),
            (
                "clone_completion_navigation",
                Box::new(crate::clone_owner::scenario_clone_completion_navigation),
            ),
            (
                "clone_hidden_status",
                Box::new(crate::clone_hidden_status::scenario_clone_hidden_status),
            ),
            (
                "clone_departed_result_owner",
                Box::new(crate::completion_visit_owner::scenario_clone_departed_result_owner),
            ),
            (
                "stash_revisit_result_owner",
                Box::new(crate::completion_visit_owner::scenario_stash_revisit_result_owner),
            ),
            (
                "remote_stash_revisit_result_owner",
                Box::new(crate::completion_visit_owner::scenario_remote_stash_revisit_result_owner),
            ),
            (
                "remote_pull_current_result_owner",
                Box::new(crate::remote_pull_result_owner::scenario_remote_pull_current_result_owner),
            ),
            (
                "remote_pull_inactive_result_owner",
                Box::new(crate::remote_pull_result_owner::scenario_remote_pull_inactive_result_owner),
            ),
            (
                "remote_pull_away_result_owner",
                Box::new(crate::remote_pull_result_owner::scenario_remote_pull_away_result_owner),
            ),
            (
                "remote_pull_revisit_result_owner",
                Box::new(crate::remote_pull_result_owner::scenario_remote_pull_revisit_result_owner),
            ),
            (
                "remote_stash_result_survives_refresh",
                Box::new(crate::remote_result_refresh::scenario_remote_stash_result_survives_refresh),
            ),
            (
                "remote_pull_result_survives_refresh",
                Box::new(crate::remote_result_refresh::scenario_remote_pull_result_survives_refresh),
            ),
            (
                "smart_commit_generation_owner",
                Box::new(crate::recovery_operations::scenario_smart_commit_generation_owner),
            ),
            (
                "smart_commit_modal_and_probe",
                Box::new(crate::recovery_operations::scenario_smart_commit_modal_and_probe),
            ),
            (
                "smart_commit_settings_consent",
                Box::new(crate::smart_commit_settings_consent::scenario_smart_commit_settings_consent),
            ),
            (
                "smart_picker_keyboard",
                Box::new(crate::smart_picker::keyboard),
            ),
            (
                "smart_picker_empty",
                Box::new(crate::smart_picker::empty),
            ),
            (
                "update_install_lifecycle",
                Box::new(crate::recovery_operations::scenario_update_install_lifecycle),
            ),
            (
                "update_notice_in_flight",
                Box::new(crate::update_notice::scenario_update_notice_in_flight),
            ),
            (
                "branch_menu_no_checkout_fallthrough",
                Box::new(crate::recovery_operations::scenario_branch_menu_no_checkout_fallthrough),
            ),
            (
                "push_failure_keeps_modal",
                Box::new(crate::recovery_operations::scenario_push_failure_keeps_modal),
            ),
            (
                "checkout_presents_backend_receipt",
                Box::new(crate::recovery_operations::scenario_checkout_presents_backend_receipt),
            ),
            (
                "cherry_pick_presents_backend_receipt",
                Box::new(crate::recovery_operations::scenario_cherry_pick_presents_backend_receipt),
            ),
            (
                "create_branch_presents_backend_receipt",
                Box::new(
                    crate::recovery_operations::scenario_create_branch_presents_backend_receipt,
                ),
            ),
            (
                "create_ref_collision_blockers",
                Box::new(crate::ref_validation::scenario_create_ref_collision_blockers),
            ),
            (
                "set_upstream_missing_blocker",
                Box::new(crate::ref_validation::scenario_set_upstream_missing_blocker),
            ),
            (
                "create_branch_execution_failure_keeps_input",
                Box::new(
                    crate::recovery_operations::scenario_create_branch_execution_failure_keeps_input,
                ),
            ),
            (
                "run_success_unrecorded_requires_acknowledgment",
                Box::new(
                    crate::recovery_operations::scenario_run_success_unrecorded_requires_acknowledgment,
                ),
            ),
            (
                "backend_cli_capability_observation",
                Box::new(
                    crate::recovery_cli_capability::scenario_backend_cli_capability_observation,
                ),
            ),
            (
                "pull_safe_dirty_intent",
                Box::new(crate::pull_freshness::scenario_pull_safe_dirty_intent),
            ),
            (
                "pull_safe_merge_intent",
                Box::new(crate::pull_freshness::scenario_pull_safe_merge_intent),
            ),
            (
                "pull_history_conflict_choice",
                Box::new(crate::pull_freshness::scenario_pull_history_conflict_choice),
            ),
            (
                "push_safe_intent",
                Box::new(crate::pull_freshness::scenario_push_safe_intent),
            ),
            (
                "entry_consistency_sidebar_push_safe_intent",
                Box::new(crate::entry_consistency::scenario_sidebar_push_safe_intent),
            ),
            (
                "entry_consistency_sidebar_publish_safe_intent",
                Box::new(crate::entry_consistency::scenario_sidebar_publish_safe_intent),
            ),
            (
                "entry_consistency_sidebar_publish_ambiguous_remote",
                Box::new(crate::entry_consistency::scenario_sidebar_publish_ambiguous_remote),
            ),
            (
                "entry_consistency_palette_pull_push_safe_intent",
                Box::new(crate::entry_consistency::scenario_palette_pull_push_safe_intent),
            ),
            (
                "entry_consistency_shortcut_pull_push_safe_intent",
                Box::new(crate::entry_consistency::scenario_shortcut_pull_push_safe_intent),
            ),
            (
                "entry_consistency_sidebar_noncurrent_pull_safe_intent",
                Box::new(crate::entry_consistency::scenario_sidebar_noncurrent_pull_safe_intent),
            ),
            (
                "pull_quiet_late_dirty_refuses",
                Box::new(crate::pull_freshness::scenario_pull_quiet_late_dirty_refuses),
            ),
            (
                "push_non_ff_refuses",
                Box::new(crate::pull_freshness::scenario_push_non_ff_refuses),
            ),
            (
                "pull_freshness_clean_updates",
                Box::new(crate::pull_freshness::scenario_pull_freshness_clean_updates),
            ),
            (
                "pull_current_up_to_date",
                Box::new(crate::pull_freshness::scenario_pull_current_up_to_date),
            ),
            (
                "pull_current_ff_only_blocker",
                Box::new(crate::pull_freshness::scenario_pull_current_ff_only_blocker),
            ),
            (
                "pull_current_slash_remote",
                Box::new(crate::pull_freshness::scenario_pull_current_slash_remote),
            ),
            (
                "pull_current_local_upstream",
                Box::new(crate::pull_freshness::scenario_pull_current_local_upstream),
            ),
            (
                "pull_freshness_branch_entries",
                Box::new(crate::pull_freshness::scenario_pull_freshness_branch_entries),
            ),
            (
                "pull_ff_only_current_tree_consistency",
                Box::new(crate::pull_freshness::scenario_pull_ff_only_current_tree_consistency),
            ),
            (
                "pull_ff_only_diverged_refusal",
                Box::new(crate::pull_freshness::scenario_pull_ff_only_diverged_refusal),
            ),
            (
                "pull_branch_slash_remote_current",
                Box::new(crate::pull_freshness::scenario_pull_branch_slash_remote_current),
            ),
            (
                "pull_branch_slash_remote_noncurrent",
                Box::new(crate::pull_freshness::scenario_pull_branch_slash_remote_noncurrent),
            ),
            (
                "pull_branch_local_upstream_current",
                Box::new(crate::pull_freshness::scenario_pull_branch_local_upstream_current),
            ),
            (
                "pull_branch_local_upstream_noncurrent",
                Box::new(crate::pull_freshness::scenario_pull_branch_local_upstream_noncurrent),
            ),
            (
                "pull_freshness_synced_waits",
                Box::new(crate::pull_freshness::scenario_pull_freshness_synced_waits),
            ),
            (
                "pull_freshness_fetch_failure",
                Box::new(crate::pull_freshness::scenario_pull_freshness_fetch_failure),
            ),
            (
                "pull_freshness_joins_held_fetch",
                Box::new(crate::pull_freshness::scenario_pull_freshness_joins_held_fetch),
            ),
            (
                "pull_freshness_external_head_invalidates",
                Box::new(crate::pull_freshness::scenario_pull_freshness_external_head_invalidates),
            ),
            (
                "pull_freshness_external_worktree_invalidates",
                Box::new(
                    crate::pull_freshness::scenario_pull_freshness_external_worktree_invalidates,
                ),
            ),
            (
                "pull_freshness_closed_modal_displaces",
                Box::new(crate::pull_freshness::scenario_pull_freshness_closed_modal_displaces),
            ),
            (
                "pull_freshness_captured_reload_drift",
                Box::new(crate::pull_freshness::scenario_pull_freshness_captured_reload_drift),
            ),
            (
                "pull_auto_stash_tracking_identity_drift",
                Box::new(crate::pull_freshness::scenario_pull_auto_stash_tracking_identity_drift),
            ),
            (
                "pull_auto_stash_remote_identity_drift",
                Box::new(crate::pull_freshness::scenario_pull_auto_stash_remote_identity_drift),
            ),
            (
                "pull_auto_stash_dirty_plan_drift",
                Box::new(crate::pull_freshness::scenario_pull_auto_stash_dirty_plan_drift),
            ),
            (
                "pull_auto_stash_restore_preview_drift",
                Box::new(crate::pull_freshness::scenario_pull_auto_stash_restore_preview_drift),
            ),
            (
                "pull_auto_stash_same_upstream_advancement",
                Box::new(crate::pull_freshness::scenario_pull_auto_stash_same_upstream_advancement),
            ),
            (
                "pull_fetch_preserves_conflict_blocker",
                Box::new(
                    crate::pull_fetch_blocker::scenario_pull_fetch_preserves_conflict_blocker,
                ),
            ),
            (
                "pull_auto_stash_success",
                Box::new(crate::recovery_pull::scenario_pull_auto_stash_success),
            ),
            (
                "pull_auto_stash_overlap_preview",
                Box::new(crate::recovery_pull::scenario_pull_auto_stash_overlap_preview),
            ),
            (
                "remote_pull_lease",
                Box::new(crate::recovery_pull::scenario_remote_pull_lease),
            ),
            (
                "remote_pull_unknown_release",
                Box::new(crate::recovery_pull::scenario_remote_pull_unknown_release),
            ),
            (
                "remote_pull_preflight_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_preflight_refusal),
            ),
            (
                "remote_pull_proxy_route_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_proxy_route_refusal),
            ),
            (
                "remote_pull_toplevel_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_toplevel_refusal),
            ),
            (
                "remote_pull_branch_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_branch_refusal),
            ),
            (
                "remote_pull_head_oid_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_head_oid_refusal),
            ),
            (
                "remote_pull_upstream_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_upstream_refusal),
            ),
            (
                "remote_pull_head_read_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_head_read_refusal),
            ),
            (
                "remote_pull_cached_preview_stale",
                Box::new(crate::recovery_pull::scenario_remote_pull_cached_preview_stale),
            ),
            (
                "remote_pull_url_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_url_refusal),
            ),
            (
                "remote_pull_dirty_refusal",
                Box::new(crate::recovery_pull::scenario_remote_pull_dirty_refusal),
            ),
            (
                "remote_pull_planning_latch",
                Box::new(crate::recovery_pull::scenario_remote_pull_planning_latch),
            ),
            (
                "pull_confirm_departure_discards_old_visit",
                Box::new(crate::recovery_pull::scenario_pull_confirm_departure_discards_old_visit),
            ),
            (
                "pull_confirm_yields_to_another_modal",
                Box::new(crate::recovery_pull::scenario_pull_confirm_yields_to_another_modal),
            ),
            (
                "pull_failure_notice_waits_for_remote_browse",
                Box::new(
                    crate::recovery_pull::scenario_pull_failure_notice_waits_for_remote_browse,
                ),
            ),
            (
                "pull_failure_notice_waits_for_app_notice",
                Box::new(
                    crate::recovery_pull::scenario_pull_failure_notice_waits_for_app_notice,
                ),
            ),
            (
                "pull_failure_notice_displacement_vs_dismissal",
                Box::new(
                    crate::recovery_pull::scenario_pull_failure_notice_displacement_vs_dismissal,
                ),
            ),
            (
                "pull_refuses_when_the_dirty_set_moved",
                Box::new(crate::recovery_pull::scenario_pull_refuses_when_the_dirty_set_moved),
            ),
            (
                "run_unknown_offers_its_reconcile",
                Box::new(crate::recovery_pull::scenario_run_unknown_offers_its_reconcile),
            ),
            (
                "pull_unknown_notice_survives_a_tab_switch",
                Box::new(
                    crate::recovery_pull::scenario_pull_unknown_notice_survives_a_tab_switch,
                ),
            ),
            (
                "pull_unknown_offers_its_reconcile",
                Box::new(crate::recovery_pull::scenario_pull_unknown_offers_its_reconcile),
            ),
            (
                "pull_failure_presents_only_the_decisive_receipt",
                Box::new(
                    crate::recovery_pull::scenario_pull_failure_presents_only_the_decisive_receipt,
                ),
            ),
            (
                "pull_completion_drops_when_its_tab_is_left",
                Box::new(crate::recovery_pull::scenario_pull_completion_drops_when_its_tab_is_left),
            ),
            (
                "pull_blocked_plan_presents_a_core_receipt",
                Box::new(crate::recovery_pull::scenario_pull_blocked_plan_presents_a_core_receipt),
            ),
            (
                "pull_presents_backend_receipts",
                Box::new(crate::recovery_pull::scenario_pull_presents_backend_receipts),
            ),
            (
                "pull_auto_stash_failure_restores",
                Box::new(crate::recovery_pull::scenario_pull_auto_stash_failure_restores),
            ),
            (
                "cleanup_presents_backend_receipt",
                Box::new(crate::recovery_operations::scenario_cleanup_presents_backend_receipt),
            ),
            (
                "cleanup_open_failure",
                Box::new(crate::recovery_operations::scenario_cleanup_open_failure),
            ),
            (
                "cleanup_partial_presentation",
                Box::new(crate::recovery_operations::scenario_cleanup_partial_presentation),
            ),
            (
                "remove_public_boundary",
                Box::new(crate::app_remove::scenario_remove_public_boundary),
            ),
            (
                "fetch_busy_label",
                Box::new(crate::busy_label::scenario_fetch_busy_label),
            ),
            (
                "slow_read_explained",
                Box::new(crate::slow_read::scenario_slow_read_explained),
            ),
            (
                "slow_write_explained",
                Box::new(crate::slow_read::scenario_slow_write_explained),
            ),
            (
                "busy_read_dismiss",
                Box::new(crate::busy_dismiss::scenario_busy_read_dismiss),
            ),
            (
                "busy_pull_progress_dismiss",
                Box::new(crate::busy_dismiss::scenario_busy_pull_progress_dismiss),
            ),
            (
                "busy_pull_revisit_request",
                Box::new(crate::busy_dismiss::scenario_busy_pull_revisit_request),
            ),
            (
                "busy_pull_merge_progress_dismiss",
                Box::new(crate::busy_dismiss::scenario_busy_pull_merge_progress_dismiss),
            ),
            (
                "busy_hidden_failure_reaches_details",
                Box::new(crate::busy_failure::scenario_busy_hidden_failure_reaches_details),
            ),
            (
                "slow_write_cleared_after_panic",
                Box::new(crate::slow_read::scenario_slow_write_cleared_after_panic),
            ),
            (
                "queue_commit_runs_after_checkout",
                Box::new(crate::op_queue::scenario_queue_commit_runs_after_checkout),
            ),
            (
                "commit_index_identity_refusal",
                Box::new(crate::op_queue::scenario_commit_index_identity_refusal),
            ),
            (
                "commit_hook_failure",
                Box::new(crate::op_queue::scenario_commit_hook_failure),
            ),
            (
                "queue_commit_confirms_changed_draft",
                Box::new(crate::op_queue::scenario_queue_commit_confirms_changed_draft),
            ),
            (
                "queue_commit_confirms_changed_staging",
                Box::new(crate::op_queue::scenario_queue_commit_confirms_changed_staging),
            ),
            (
                "queue_commit_consumes_origin_draft",
                Box::new(crate::op_queue::scenario_queue_commit_consumes_origin_draft),
            ),
            (
                "queue_commit_rechecks_staging_on_confirm",
                Box::new(crate::op_queue::scenario_queue_commit_rechecks_staging_on_confirm),
            ),
            (
                "queue_commit_refuses_late_merge",
                Box::new(crate::op_queue::scenario_queue_commit_refuses_late_merge),
            ),
            (
                "queue_commit_detached_successor",
                Box::new(crate::op_queue::scenario_queue_commit_detached_successor),
            ),
            (
                "queue_replans_merge_after_predecessor",
                Box::new(crate::op_queue::scenario_queue_replans_merge_after_predecessor),
            ),
            (
                "queue_confirm_departure_requeues",
                Box::new(crate::op_queue::scenario_queue_confirm_departure_requeues),
            ),
            (
                "queue_merge_conflict_trips_successors",
                Box::new(crate::op_queue::scenario_queue_merge_conflict_trips_successors),
            ),
            (
                "queue_merge_into_keeps_frozen_target",
                Box::new(crate::op_queue::scenario_queue_merge_into_keeps_frozen_target),
            ),
            (
                "queue_drag_merge_while_busy",
                Box::new(crate::op_queue::scenario_queue_drag_merge_while_busy),
            ),
            (
                "queue_menu_merge_while_busy",
                Box::new(crate::op_queue::scenario_queue_menu_merge_while_busy),
            ),
            (
                "queue_confirm_blocked_merge_refuses",
                Box::new(crate::op_queue::scenario_queue_confirm_blocked_merge_refuses),
            ),
            (
                "queue_merge_plan_after_departure",
                Box::new(crate::op_queue::scenario_queue_merge_plan_after_departure),
            ),
            (
                "queue_merge_refusal_is_shown",
                Box::new(crate::op_queue::scenario_queue_merge_refusal_is_shown),
            ),
            (
                "queue_waits_while_input_focused",
                Box::new(crate::op_queue::scenario_queue_waits_while_input_focused),
            ),
            (
                "queue_runs_in_order",
                Box::new(crate::op_queue::scenario_queue_runs_in_order),
            ),
            (
                "queue_trip_lists_cancelled",
                Box::new(crate::op_queue::scenario_queue_trip_lists_cancelled),
            ),
            (
                "queue_confirms_a_warned_plan",
                Box::new(crate::op_queue::scenario_queue_confirms_a_warned_plan),
            ),
            (
                "queue_strip_owner_only",
                Box::new(crate::op_queue::scenario_queue_strip_owner_only),
            ),
            (
                "queue_skips_auto_fetch",
                Box::new(crate::op_queue::scenario_queue_skips_auto_fetch),
            ),
            (
                "queue_dismissed_input_modal_releases_head",
                Box::new(crate::op_queue::scenario_queue_dismissed_input_modal_releases_head),
            ),
            (
                "queue_rejects_during_untracked_write",
                Box::new(crate::op_queue::scenario_queue_rejects_during_untracked_write),
            ),
            (
                "queue_resumes_after_reconcile",
                Box::new(crate::op_queue::scenario_queue_resumes_after_reconcile),
            ),
            (
                "queue_accepts_after_idle_fetch",
                Box::new(crate::op_queue::scenario_queue_accepts_after_idle_fetch),
            ),
            (
                "queue_checkout_transient_blocker_clears",
                Box::new(crate::op_queue::scenario_queue_checkout_transient_blocker_clears),
            ),
            (
                "queue_busy_checkout_preserves_local_changes",
                Box::new(crate::op_queue::scenario_queue_busy_checkout_preserves_local_changes),
            ),
            (
                "queue_checkout_missing_ref_refuses",
                Box::new(crate::op_queue::scenario_queue_checkout_missing_ref_refuses),
            ),
            (
                "queue_checkout_linked_worktree_refuses",
                Box::new(crate::op_queue::scenario_queue_checkout_linked_worktree_refuses),
            ),
            (
                "queue_commit_checkout_busy_replans",
                Box::new(crate::op_queue::scenario_queue_commit_checkout_busy_replans),
            ),
            (
                "queue_dirty_commit_enter_refuses",
                Box::new(crate::op_queue::scenario_queue_dirty_commit_enter_refuses),
            ),
            (
                "queue_refuses_a_blocked_checkout",
                Box::new(crate::op_queue::scenario_queue_refuses_a_blocked_checkout),
            ),
            (
                "slow_write_snapshot",
                Box::new(crate::slow_read::scenario_slow_write_snapshot),
            ),
            (
                "slow_write_conflict_continue",
                Box::new(crate::slow_read::scenario_slow_write_conflict_continue),
            ),
            (
                "worktree_size_not_explained",
                Box::new(crate::slow_read::scenario_worktree_size_not_explained),
            ),
            (
                "toast_dismiss",
                Box::new(crate::toast_dismiss::scenario_toast_dismiss),
            ),
            (
                "toast_tooltip_lifetime",
                Box::new(crate::toast_dismiss::scenario_toast_tooltip_lifetime),
            ),
            (
                "toast_tooltip_scroll",
                Box::new(crate::toast_dismiss::scenario_toast_tooltip_scroll),
            ),
            (
                "toast_stack_budget",
                Box::new(crate::toast_stack_budget::scenario_toast_stack_budget),
            ),
            (
                "toast_stack_budget_large_zoom",
                Box::new(crate::toast_stack_budget::scenario_toast_stack_budget_large_zoom),
            ),
            (
                "toast_stack_budget_compact_editor",
                Box::new(crate::toast_stack_budget::scenario_toast_stack_budget_compact_editor),
            ),
            (
                "toast_live_status",
                Box::new(crate::feedback_status::scenario_toast_live_status),
            ),
            (
                "diff_header_identity",
                Box::new(crate::feedback_status::scenario_diff_header_identity),
            ),
            (
                "clone_progress_status",
                Box::new(crate::clone_progress_status::scenario_clone_progress_status),
            ),
            (
                "worktree_inspection_actions",
                Box::new(crate::worktree_inspection_actions::scenario),
            ),
            (
                "fetch_failure_oplog",
                Box::new(crate::busy_label::scenario_fetch_failure_reaches_the_oplog),
            ),
            (
                "oplog_append_failure_visible",
                Box::new(crate::busy_label::scenario_oplog_append_failure_is_visible),
            ),
            (
                "fetch_changed_ref_recording_failure",
                Box::new(
                    crate::fetch_recording_failure::scenario_fetch_changed_ref_recording_failure,
                ),
            ),
            (
                "oplog_append_failure_conflict_stopped",
                Box::new(crate::conflict_stopped::scenario_oplog_append_failure_conflict_stopped),
            ),
            (
                "conflict_continue_stopped_result_owner",
                Box::new(crate::conflict_stopped::scenario_conflict_continue_stopped_result_owner),
            ),
            (
                "pr_merge_delete_branch_no_default",
                Box::new(crate::squash_safe_default::pr_merge_delete_branch_no_default),
            ),
            (
                "pr_merge_card_scope",
                Box::new(crate::squash_safe_default::pr_merge_card_scope),
            ),
            (
                "pr_merge_write_lease",
                Box::new(crate::busy_label::scenario_pr_merge_holds_the_write_lease),
            ),
            (
                "pr_merge_admission_keeps_modal",
                Box::new(crate::busy_label::scenario_pr_merge_admission_keeps_the_modal),
            ),
            (
                "merge_plan_latch",
                Box::new(crate::busy_label::scenario_merge_plan_latches_planning),
            ),
            (
                "delete_branch_plan_latch",
                Box::new(crate::busy_label::scenario_delete_branch_plan_latches_planning),
            ),
            (
                "editor_save_admission",
                Box::new(crate::app_writer_admission::scenario_editor_save_admission),
            ),
            (
                "editor_save_buffer_identity",
                Box::new(crate::app_writer_admission::scenario_editor_save_buffer_identity),
            ),
            (
                "conflict_merge_stopped_toast",
                Box::new(crate::conflict_stopped::scenario_conflict_merge_stopped_toast),
            ),
            (
                "conflict_rebase_stopped_toast",
                Box::new(crate::conflict_stopped::scenario_conflict_rebase_stopped_toast),
            ),
            (
                "branch_menu_orphan_delete",
                Box::new(crate::state_labels::branch_menu_orphan_delete),
            ),
            (
                "editor_external_change_banner",
                Box::new(crate::app_writer_admission::scenario_editor_external_change_banner),
            ),
            (
                "editor_banner_rename_and_save",
                Box::new(crate::app_writer_admission::scenario_editor_banner_rename_and_save),
            ),
            (
                "conflict_continue_cache",
                Box::new(crate::conflict_continue_cache::scenario_conflict_continue_cache),
            ),
            (
                "conflict_manual_reset_guard",
                Box::new(crate::conflict_controls::scenario_conflict_manual_reset_guard),
            ),
            (
                "conflict_partial_resume",
                Box::new(crate::conflict_controls::scenario_conflict_partial_resume),
            ),
            (
                "conflict_leave_and_return",
                Box::new(crate::conflict_controls::scenario_conflict_leave_and_return),
            ),
            (
                "conflict_continue_approval_replacement_queued",
                Box::new(crate::conflict_continue_approval::scenario_conflict_continue_approval_replacement_queued),
            ),
            (
                "conflict_continue_approval_stage0_queued",
                Box::new(crate::conflict_continue_approval::scenario_conflict_continue_approval_stage0_queued),
            ),
            (
                "conflict_continue_approval_modal_revision_replaced",
                Box::new(crate::conflict_continue_approval::scenario_conflict_continue_approval_modal_revision_replaced),
            ),
            (
                "conflict_continue_approval_unrelated_replan",
                Box::new(crate::conflict_continue_approval::scenario_conflict_continue_approval_unrelated_replan),
            ),
            (
                "conflict_draft_persist_failure",
                Box::new(crate::conflict_draft_persist_failure::scenario_conflict_draft_persist_failure),
            ),
            (
                "conflict_draft_flush_boundaries",
                Box::new(crate::conflict_draft_persist_failure::scenario_conflict_draft_flush_boundaries),
            ),
            (
                "conflict_draft_shutdown",
                Box::new(crate::conflict_draft_barrier_edges::scenario_conflict_draft_shutdown),
            ),
            (
                "conflict_draft_repaired_actions",
                Box::new(crate::conflict_draft_barrier_edges::scenario_conflict_draft_repaired_actions),
            ),
            (
                "conflict_continue_presentation",
                Box::new(crate::conflict_continue_approval::scenario_conflict_continue_presentation),
            ),
            (
                "conflict_draft_revision_audit",
                Box::new(crate::conflict_draft_revision::scenario_conflict_draft_revision_audit),
            ),
            (
                "conflict_draft_revision_append_failure",
                Box::new(crate::conflict_draft_revision::scenario_conflict_draft_revision_append_failure),
            ),
            (
                "conflict_draft_revision_same_revision",
                Box::new(crate::conflict_draft_revision::scenario_conflict_draft_revision_same_revision),
            ),
            (
                "conflict_draft_revision_unrelated_index",
                Box::new(crate::conflict_draft_revision::scenario_conflict_draft_revision_unrelated_index),
            ),
            (
                "conflict_draft_revision_saved_stale",
                Box::new(crate::conflict_draft_revision::scenario_conflict_draft_revision_saved_stale),
            ),
            (
                "conflict_draft_revision_saved_legacy",
                Box::new(crate::conflict_draft_revision::scenario_conflict_draft_revision_saved_legacy),
            ),
            (
                "conflict_draft_corrupt_restore",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_corrupt_restore),
            ),
            (
                "conflict_draft_restore_io_failure",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_restore_io_failure),
            ),
            (
                "conflict_draft_stale_restore_failure",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_stale_restore_failure),
            ),
            (
                "conflict_draft_stale_restore_async",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_stale_restore_async),
            ),
            (
                "conflict_draft_fresh_restore_failure",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_fresh_restore_failure),
            ),
            (
                "conflict_draft_continue_restore_admission",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_continue_restore_admission),
            ),
            (
                "conflict_draft_recovered_save_frozen_payload",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_recovered_save_frozen_payload),
            ),
            (
                "conflict_draft_stale_restore_success",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_stale_restore_success),
            ),
            (
                "conflict_draft_abort_restore_admission",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_abort_restore_admission),
            ),
            (
                "conflict_draft_cleared_handoff",
                Box::new(crate::conflict_draft_revision::scenario_conflict_draft_cleared_handoff),
            ),
            (
                "conflict_draft_stale_handoff",
                Box::new(crate::conflict_draft_revision::scenario_conflict_draft_stale_handoff),
            ),
            (
                "conflict_open_fresh",
                Box::new(crate::conflict_open::scenario_conflict_open_fresh),
            ),
            (
                "conflict_open_second_tab",
                Box::new(crate::conflict_open::scenario_conflict_open_second_tab),
            ),
            (
                "conflict_launch",
                Box::new(crate::conflict_open::scenario_conflict_launch),
            ),
            (
                "conflict_draft_inspection_empty_legacy",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_inspection_empty_legacy),
            ),
            (
                "conflict_draft_inspection_unrecorded_restart",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_inspection_unrecorded_restart),
            ),
            (
                "conflict_draft_inspection_scope_change",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_inspection_scope_change),
            ),
            (
                "conflict_draft_inspection_gitdir_replaced",
                Box::new(crate::conflict_draft_inspection::scenario_conflict_draft_inspection_gitdir_replaced),
            ),
            (
                "conflict_reset_disarm",
                Box::new(crate::conflict_controls::scenario_conflict_reset_disarm),
            ),
            (
                "conflict_arrow_checkboxes",
                Box::new(crate::conflict_controls::scenario_conflict_arrow_checkboxes),
            ),
            (
                "conflict_merged_passthrough",
                Box::new(crate::conflict_marker_width::scenario_conflict_merged_passthrough),
            ),
            (
                "conflict_marker_width",
                Box::new(crate::conflict_marker_width::scenario_conflict_marker_width),
            ),
            (
                "conflict_width_draft_recovery",
                Box::new(crate::conflict_marker_width::scenario_conflict_width_draft_recovery),
            ),
            (
                "conflict_width_recovery_survives_other_file_save",
                Box::new(
                    crate::conflict_marker_width::scenario_conflict_width_recovery_survives_other_file_save,
                ),
            ),
            (
                "conflict_result_segment",
                Box::new(crate::conflict_controls::scenario_conflict_result_segment),
            ),
            (
                "conflict_progressive_add_add",
                Box::new(crate::conflict_controls::scenario_conflict_progressive_add_add),
            ),
            (
                "conflict_file_selection",
                Box::new(crate::conflict_file_selection::scenario_conflict_file_selection),
            ),
            (
                "conflict_text_keyboard",
                Box::new(crate::conflict_text_keyboard::scenario_conflict_text_keyboard),
            ),
            (
                "conflict_resolver_rebuild",
                Box::new(crate::conflict_text_keyboard::scenario_conflict_resolver_rebuild),
            ),
            (
                "conflict_resolver_roles",
                Box::new(crate::conflict_text_keyboard::scenario_conflict_resolver_roles),
            ),
            (
                "modal_polish_editor_fs",
                Box::new(crate::modal_polish_b::scenario_modal_polish_editor_fs),
            ),
            (
                "modal_polish_remote",
                Box::new(crate::modal_polish_b::scenario_modal_polish_remote),
            ),
            (
                "modal_polish_stash_after",
                Box::new(crate::modal_polish_b::scenario_modal_polish_stash_after),
            ),
            (
                "trust_failure_feedback",
                Box::new(crate::modal_polish_b::scenario_trust_failure_feedback),
            ),
            (
                "open_repository_picker_localized",
                Box::new(crate::open_picker::scenario_open_repository_picker_localized),
            ),
            (
                "open_repository_picker_error",
                Box::new(crate::open_picker::scenario_open_repository_picker_error),
            ),
            (
                "open_repository_picker_home_owner",
                Box::new(crate::open_picker::scenario_open_repository_picker_home_owner),
            ),
            (
                "open_repository_picker_frozen_owner",
                Box::new(crate::open_picker::scenario_open_repository_picker_frozen_owner),
            ),
            (
                "worktree_folder_picker",
                Box::new(crate::worktree_folder_picker::scenario_worktree_folder_picker),
            ),
            (
                "worktree_modal_prefill",
                Box::new(crate::worktree_folder_picker::scenario_worktree_modal_prefill),
            ),
            (
                "worktree_create_default_name",
                Box::new(crate::worktree_default_name::scenario_worktree_create_default_name),
            ),
            (
                "repo_health_proposal",
                Box::new(crate::repo_health::scenario_repo_health_proposal),
            ),
            ("analyze_a11y_controls", Box::new(crate::analyze_a11y::scenario_analyze_a11y_controls)),
            ("analyze_a11y_risk", Box::new(crate::analyze_a11y::scenario_analyze_a11y_risk)),
            ("analyze_a11y_truncation", Box::new(crate::analyze_a11y::scenario_analyze_a11y_truncation)),
            ("analyze_a11y_help", Box::new(crate::analyze_a11y::scenario_analyze_a11y_help)),
            ("analyze_ownership_author", Box::new(crate::analyze_a11y::scenario_analyze_ownership_author)),
            (
                "refusal_reasons",
                Box::new(crate::refusal_reasons::scenario_refusal_reasons),
            ),
            (
                "diff_highlight_once",
                Box::new(crate::diff_highlight::scenario_diff_highlight_once),
            ),
            (
                "diff_highlight_stale",
                Box::new(crate::diff_highlight::scenario_diff_highlight_stale),
            ),
            (
                "commit_diff_off_thread",
                Box::new(crate::diff_highlight::scenario_commit_diff_off_thread),
            ),
            (
                "binary_diff_prepared",
                Box::new(crate::diff_highlight::scenario_binary_diff_prepared),
            ),
            (
                "binary_diff_owner_transitions",
                Box::new(crate::binary_diff_owner::scenario_binary_diff_owner_transitions),
            ),
            (
                "binary_diff_worktree_owner",
                Box::new(crate::binary_diff_owner::scenario_binary_diff_worktree_owner),
            ),
            (
                "binary_diff_linked_empty_revisit",
                Box::new(crate::binary_diff_owner::scenario_binary_diff_linked_empty_revisit),
            ),
            (
                "commit_pdf_sources",
                Box::new(crate::commit_pdf::scenario_commit_pdf_sources),
            ),
            (
                "commit_pdf_staged_rename",
                Box::new(crate::commit_pdf::scenario_commit_pdf_staged_rename),
            ),
            (
                "commit_pdf_owners",
                Box::new(crate::commit_pdf::scenario_commit_pdf_owners),
            ),
            (
                "commit_pdf_continuous_sides",
                Box::new(crate::commit_pdf::scenario_commit_pdf_continuous_sides),
            ),
            (
                "commit_pdf_sync_scrolling",
                Box::new(crate::pdf_sync::scenario_commit_pdf_sync_scrolling),
            ),
            (
                "commit_pdf_sync_lifecycle",
                Box::new(crate::pdf_sync::scenario_commit_pdf_sync_lifecycle),
            ),
            (
                "commit_pdf_fit_width",
                Box::new(crate::pdf_fit::scenario_commit_pdf_fit_width),
            ),
            (
                "pdf_focus_retirement",
                Box::new(crate::pdf_focus_retirement::scenario_pdf_focus_retirement),
            ),
            (
                "pdf_focus_retirement_preserves_input",
                Box::new(crate::pdf_focus_retirement::scenario_pdf_focus_retirement_preserves_input),
            ),
            (
                "pdf_media_sources",
                Box::new(crate::pdf_media_bounds::scenario_pdf_media_sources),
            ),
            (
                "pdf_media_bounds",
                Box::new(crate::pdf_media_bounds::scenario_pdf_media_bounds),
            ),
            (
                "pdf_media_symlink",
                Box::new(crate::pdf_media_bounds::scenario_pdf_media_symlink),
            ),
            (
                "conflict_preview_no_render_io",
                Box::new(crate::recovery_diff_presentation::scenario_conflict_preview_no_render_io),
            ),
            (
                "conflict_preview_owner_transitions",
                Box::new(crate::recovery_diff_presentation::scenario_conflict_preview_owner_transitions),
            ),
            (
                "conflict_preview_read_retry",
                Box::new(crate::recovery_diff_presentation::scenario_conflict_preview_read_retry),
            ),
            (
                "conflict_side_pair_image",
                Box::new(crate::side_pair::scenario_conflict_side_pair_image),
            ),
            (
                "conflict_side_pair_pdf",
                Box::new(crate::side_pair::scenario_conflict_side_pair_pdf),
            ),
            (
                "conflict_side_pair_modify_delete",
                Box::new(crate::side_pair::scenario_conflict_side_pair_modify_delete),
            ),
            (
                "conflict_save_boundary",
                Box::new(crate::app_conflict::scenario_conflict_save_boundary),
            ),
            (
                "conflict_repository_lock_continue",
                Box::new(crate::repository_lock::scenario_conflict_repository_lock_continue),
            ),
            (
                "conflict_repository_lock_skip",
                Box::new(crate::repository_lock::scenario_conflict_repository_lock_skip),
            ),
            (
                "repository_lock_silent_auto_fetch",
                Box::new(crate::repository_lock::scenario_repository_lock_silent_auto_fetch),
            ),
            (
                "repository_lock_stage_receipt",
                Box::new(crate::repository_lock::scenario_repository_lock_stage_receipt),
            ),
            (
                "repository_lock_queue_retry",
                Box::new(crate::repository_lock::scenario_repository_lock_queue_retry),
            ),
            (
                "repository_lock_user_receipts",
                Box::new(crate::repository_lock::scenario_repository_lock_user_receipts),
            ),
            (
                "repository_lock_admission_recording_failure",
                Box::new(crate::repository_lock::scenario_repository_lock_admission_recording_failure),
            ),
            (
                "repository_lock_silent_pr_refresh",
                Box::new(crate::pr_viewed::scenario_repository_lock_silent_pr_refresh),
            ),
            (
                "repository_lock_legacy_plan_recording_failure",
                Box::new(crate::conflict_abort_slot::scenario_legacy_plan_recording_failure),
            ),
            (
                "conflict_continue_refusal_result_owner",
                Box::new(crate::app_conflict::scenario_conflict_continue_refusal_result_owner),
            ),
            (
                "conflict_dir_file_boundary",
                Box::new(crate::app_conflict::scenario_conflict_dir_file_boundary),
            ),
            (
                "conflict_delete_routes_en",
                Box::new(crate::app_conflict::scenario_conflict_delete_routes_en),
            ),
            (
                "conflict_delete_routes_ja",
                Box::new(crate::app_conflict::scenario_conflict_delete_routes_ja),
            ),
            (
                "conflict_family_result_owner",
                Box::new(crate::app_conflict::scenario_conflict_family_result_owner),
            ),
            (
                "conflict_continue_failure_result_owner",
                Box::new(crate::app_conflict::scenario_conflict_continue_failure_result_owner),
            ),
            (
                "operation_strip_startup",
                Box::new(crate::app_conflict::scenario_operation_strip_startup),
            ),
            (
                "operation_strip_hidden_in_editor",
                Box::new(crate::app_conflict::scenario_operation_strip_hidden_in_editor),
            ),
            (
                "conflict_abort_escape_focus",
                Box::new(crate::conflict_abort_slot::scenario_conflict_abort_escape_focus),
            ),
            (
                "conflict_abort_superseded_reload",
                Box::new(crate::conflict_abort_slot::scenario_conflict_abort_superseded_reload),
            ),
            (
                "cherry_pick_abort_sequence",
                Box::new(crate::conflict_abort_slot::scenario_cherry_pick_abort_sequence),
            ),
            (
                "remote_browse_viewport",
                Box::new(crate::modal_viewports::scenario_remote_browse_viewport),
            ),
            (
                "trust_prompt_viewport",
                Box::new(crate::modal_viewports::scenario_trust_prompt_viewport),
            ),
            (
                "remote_browse_escape_focus",
                Box::new(crate::remote_browse_focus::scenario_remote_browse_escape_focus),
            ),
            (
                "remote_browse_keyboard_rows",
                Box::new(crate::remote_browse_focus::scenario_remote_browse_keyboard_rows),
            ),
            (
                "remote_browse_keyboard_read_ownership",
                Box::new(crate::remote_browse_focus::scenario_remote_browse_keyboard_read_ownership),
            ),
            (
                "remote_browse_dark_elevation",
                Box::new(crate::remote_browse_focus::scenario_remote_browse_dark_elevation),
            ),
            (
                "palette_push_modal_keys",
                Box::new(crate::overlay_focus::scenario_palette_push_modal_keys),
            ),
            (
                "palette_selection_scroll",
                Box::new(crate::overlay_focus::scenario_palette_selection_scroll),
            ),
            (
                "palette_rows_ax_action",
                Box::new(crate::palette_ax::scenario_palette_rows_ax_action),
            ),
            (
                "palette_conflict_admission",
                Box::new(crate::palette_ax::scenario_palette_conflict_admission),
            ),
            (
                "palette_viewport",
                Box::new(crate::overlay_focus::scenario_palette_viewport),
            ),
            (
                "palette_highlight",
                Box::new(crate::palette_polish::scenario_palette_highlight),
            ),
            (
                "palette_shortcut_glyphs",
                Box::new(crate::palette_polish::scenario_palette_shortcut_glyphs),
            ),
            (
                "palette_sections",
                Box::new(crate::palette_polish::scenario_palette_sections),
            ),
            (
                "settings_close_returns_focus",
                Box::new(crate::overlay_focus::scenario_settings_close_returns_focus),
            ),
            (
                "settings_focus_trap",
                Box::new(crate::overlay_focus::scenario_settings_focus_trap),
            ),
            (
                "settings_hidden_return_target",
                Box::new(crate::overlay_focus::scenario_settings_hidden_return_target),
            ),
            (
                "settings_provider_keys",
                Box::new(crate::settings_controls::scenario_settings_provider_keys),
            ),
            (
                "settings_theme_select_ax",
                Box::new(crate::settings_appearance_ax::scenario_settings_theme_select_ax),
            ),
            (
                "settings_zoom_stepper_ax",
                Box::new(crate::settings_appearance_ax::scenario_settings_zoom_stepper_ax),
            ),
            (
                "settings_compact_threshold_ax",
                Box::new(crate::settings_appearance_ax::scenario_settings_compact_threshold_ax),
            ),
            (
                "settings_analyze_ignore_feedback",
                Box::new(crate::settings_controls::scenario_settings_analyze_ignore_feedback),
            ),
            (
                "settings_analyze_reset_confirm",
                Box::new(crate::settings_analyze::scenario_settings_analyze_reset_confirm),
            ),
            (
                "settings_analyze_portable_reset",
                Box::new(crate::settings_analyze::scenario_settings_analyze_portable_reset),
            ),
            (
                "settings_analyze_reset_preserves_plan",
                Box::new(crate::settings_analyze::scenario_settings_analyze_reset_preserves_plan),
            ),
            (
                "settings_help_text",
                Box::new(crate::settings_analyze::scenario_settings_help_text),
            ),
            (
                "settings_analyze_save_state",
                Box::new(crate::settings_analyze::scenario_settings_analyze_save_state),
            ),
            (
                "settings_switches",
                Box::new(crate::settings_switches::scenario_settings_switches),
            ),
            (
                "settings_scrollbar_drag",
                Box::new(crate::surface_scrollbars::scenario_settings_scrollbar_drag),
            ),
            (
                "settings_zoom_bound",
                Box::new(crate::settings_switches::scenario_settings_zoom_bound),
            ),
            (
                "zoom_200_operable",
                Box::new(crate::zoom_200::scenario_zoom_200_operable),
            ),
            (
                "settings_groups",
                Box::new(crate::settings_switches::scenario_settings_groups),
            ),
            (
                "tab_panels",
                Box::new(crate::tab_panels::scenario_tab_panels),
            ),
            (
                "sidebar_keyboard_collapse",
                Box::new(crate::sidebar_groups::scenario_sidebar_keyboard_collapse),
            ),
            (
                "sidebar_keyboard_collapse_card",
                Box::new(crate::sidebar_groups::scenario_sidebar_keyboard_collapse_card),
            ),
            (
                "sidebar_filtered_disclosure",
                Box::new(crate::sidebar_groups::scenario_sidebar_filtered_disclosure),
            ),
            (
                "sidebar_rows",
                Box::new(crate::sidebar_rows::scenario_sidebar_rows),
            ),
            (
                "sidebar_rows_short",
                Box::new(crate::sidebar_rows::scenario_sidebar_rows_short),
            ),
            (
                "sidebar_rows_open_short",
                Box::new(crate::sidebar_rows::scenario_sidebar_rows_open_short),
            ),
            (
                "sidebar_rows_right_click",
                Box::new(crate::sidebar_rows::scenario_sidebar_rows_right_click),
            ),
            (
                "sidebar_rows_scroll",
                Box::new(crate::sidebar_rows::scenario_sidebar_rows_scroll),
            ),
            (
                "sidebar_rows_keys",
                Box::new(crate::sidebar_rows::scenario_sidebar_rows_keys),
            ),
            (
                "stash_row_identity",
                Box::new(crate::sidebar_identity::scenario_stash_row_identity),
            ),
            (
                "tag_unloaded_target",
                Box::new(crate::sidebar_identity::scenario_tag_unloaded_target),
            ),
            (
                "remote_group_header_fit",
                Box::new(crate::sidebar_identity::scenario_remote_group_header_fit),
            ),
            (
                "tag_unloaded_target_solo",
                Box::new(crate::sidebar_identity::scenario_tag_unloaded_target_solo),
            ),
            (
                "sidebar_group_reveal_lifetime",
                Box::new(crate::sidebar_identity::scenario_sidebar_group_reveal_lifetime),
            ),
            (
                "tag_unloaded_target_en_light_100_1392",
                Box::new(|cx| tag_cell(cx, En, "apple-light", 1., (1392., 883.))),
            ),
            (
                "tag_unloaded_target_en_light_167_1000",
                Box::new(|cx| tag_cell(cx, En, "apple-light", 1.67, (1000., 720.))),
            ),
            (
                "tag_unloaded_target_en_light_167_1392",
                Box::new(|cx| tag_cell(cx, En, "apple-light", 1.67, (1392., 883.))),
            ),
            (
                "tag_unloaded_target_en_dark_100_1000",
                Box::new(|cx| tag_cell(cx, En, "apple-dark", 1., (1000., 720.))),
            ),
            (
                "tag_unloaded_target_en_dark_100_1392",
                Box::new(|cx| tag_cell(cx, En, "apple-dark", 1., (1392., 883.))),
            ),
            (
                "tag_unloaded_target_en_dark_167_1000",
                Box::new(|cx| tag_cell(cx, En, "apple-dark", 1.67, (1000., 720.))),
            ),
            (
                "tag_unloaded_target_en_dark_167_1392",
                Box::new(|cx| tag_cell(cx, En, "apple-dark", 1.67, (1392., 883.))),
            ),
            (
                "tag_unloaded_target_ja_light_100_1000",
                Box::new(|cx| tag_cell(cx, Ja, "apple-light", 1., (1000., 720.))),
            ),
            (
                "tag_unloaded_target_ja_light_100_1392",
                Box::new(|cx| tag_cell(cx, Ja, "apple-light", 1., (1392., 883.))),
            ),
            (
                "tag_unloaded_target_ja_light_167_1000",
                Box::new(|cx| tag_cell(cx, Ja, "apple-light", 1.67, (1000., 720.))),
            ),
            (
                "tag_unloaded_target_ja_light_167_1392",
                Box::new(|cx| tag_cell(cx, Ja, "apple-light", 1.67, (1392., 883.))),
            ),
            (
                "tag_unloaded_target_ja_dark_100_1000",
                Box::new(|cx| tag_cell(cx, Ja, "apple-dark", 1., (1000., 720.))),
            ),
            (
                "tag_unloaded_target_ja_dark_100_1392",
                Box::new(|cx| tag_cell(cx, Ja, "apple-dark", 1., (1392., 883.))),
            ),
            (
                "tag_unloaded_target_ja_dark_167_1000",
                Box::new(|cx| tag_cell(cx, Ja, "apple-dark", 1.67, (1000., 720.))),
            ),
            (
                "tag_unloaded_target_ja_dark_167_1392",
                Box::new(|cx| tag_cell(cx, Ja, "apple-dark", 1.67, (1392., 883.))),
            ),
            (
                "remote_group_header_fit_en_light_167",
                Box::new(|cx| group_cell(cx, En, "apple-light", 1.67)),
            ),
            (
                "remote_group_header_fit_en_dark_100",
                Box::new(|cx| group_cell(cx, En, "apple-dark", 1.)),
            ),
            (
                "remote_group_header_fit_en_dark_167",
                Box::new(|cx| group_cell(cx, En, "apple-dark", 1.67)),
            ),
            (
                "remote_group_header_fit_ja_light_100",
                Box::new(|cx| group_cell(cx, Ja, "apple-light", 1.)),
            ),
            (
                "remote_group_header_fit_ja_light_167",
                Box::new(|cx| group_cell(cx, Ja, "apple-light", 1.67)),
            ),
            (
                "remote_group_header_fit_ja_dark_100",
                Box::new(|cx| group_cell(cx, Ja, "apple-dark", 1.)),
            ),
            (
                "remote_group_header_fit_ja_dark_167",
                Box::new(|cx| group_cell(cx, Ja, "apple-dark", 1.67)),
            ),
            (
                "repo_tab_panels",
                Box::new(crate::tab_panels::scenario_repo_tab_panels),
            ),
            (
                "settings_scroll_stays_in_overlay",
                Box::new(crate::overlay_focus::scenario_settings_scroll_stays_in_overlay),
            ),
            (
                "pr_fields_escape_focus",
                Box::new(crate::pr_fields_focus::scenario_pr_fields_escape_focus),
            ),
            (
                "pr_same_number",
                Box::new(crate::pr_same_number::scenario_pr_same_number),
            ),
            ("pr_viewed", Box::new(crate::pr_viewed::scenario_pr_viewed)),
            ("pr_lane_keyboard", Box::new(crate::pr_lane_keys::scenario_pr_lane_keyboard)),
            ("pr_lane_selected", Box::new(crate::pr_lane_keys::scenario_pr_lane_selected)),
            ("pr_lane_count", Box::new(crate::pr_lane_keys::scenario_pr_lane_count)),
            (
                "pr_detail_adaptive",
                Box::new(crate::pr_detail_adaptive::scenario_pr_detail_adaptive),
            ),
            (
                "pr_detail_lifecycle",
                Box::new(crate::pr_detail_adaptive::scenario_pr_detail_lifecycle),
            ),
            ("pr_detail_a11y_1255", Box::new(crate::pr_detail_a11y::scenario_pr_detail_a11y_1255)),
            ("pr_detail_a11y_1256", Box::new(crate::pr_detail_a11y::scenario_pr_detail_a11y_1256)),
            ("pr_detail_a11y_1257", Box::new(crate::pr_detail_a11y::scenario_pr_detail_a11y_1257)),
            ("pr_detail_a11y_1259", Box::new(crate::pr_detail_a11y::scenario_pr_detail_a11y_1259)),
            ("pr_detail_a11y_1260_en", Box::new(crate::pr_detail_a11y::scenario_pr_detail_a11y_1260_en)),
            ("pr_detail_a11y_1260_ja", Box::new(crate::pr_detail_a11y::scenario_pr_detail_a11y_1260_ja)),
            (
                "pr_diff_extent_unified",
                Box::new(crate::pr_diff_extent::scenario_pr_diff_extent_unified),
            ),
            (
                "pr_diff_extent_split",
                Box::new(crate::pr_diff_extent::scenario_pr_diff_extent_split),
            ),
            (
                "pr_diff_extent_reflow",
                Box::new(crate::pr_diff_extent_invalidation::scenario_pr_diff_extent_reflow),
            ),
            (
                "pr_diff_extent_conflict_end_fill",
                Box::new(crate::pr_diff_extent_invalidation::scenario_pr_diff_extent_conflict_end_fill),
            ),
            (
                "pr_diff_extent_tail_collapse",
                Box::new(crate::pr_diff_extent_invalidation::scenario_pr_diff_extent_tail_collapse),
            ),
            (
                "pr_diff_extent_threads",
                Box::new(crate::pr_diff_extent_invalidation::scenario_pr_diff_extent_threads),
            ),
            (
                "pr_diff_extent_source_recolor",
                Box::new(crate::pr_diff_extent_invalidation::scenario_pr_diff_extent_source_recolor),
            ),
            (
                "pr_diff_extent_same_selection",
                Box::new(crate::pr_diff_extent_same_selection::scenario_pr_diff_extent_same_selection),
            ),
            (
                "editor_diff_extent_unchanged_reload",
                Box::new(crate::editor_diff_extent_reload::scenario_editor_diff_extent_unchanged_reload),
            ),
            (
                "editor_pdf_readonly",
                Box::new(crate::editor_pdf::scenario_editor_pdf_readonly),
            ),
            (
                "editor_pdf_fit_width",
                Box::new(crate::pdf_fit::scenario_editor_pdf_fit_width),
            ),
            (
                "editor_pdf_keyboard",
                Box::new(crate::editor_pdf_keyboard::scenario_editor_pdf_keyboard),
            ),
            (
                "editor_pdf_scrollbar_drag",
                Box::new(crate::editor_pdf_scrollbars::scenario_editor_pdf_scrollbar_drag),
            ),
            (
                "editor_details_controls",
                Box::new(crate::editor_details::scenario_editor_details_controls),
            ),
            (
                "editor_details_documents",
                Box::new(crate::editor_details::scenario_editor_details_documents),
            ),
            (
                "editor_details_motion",
                Box::new(crate::editor_details::scenario_editor_details_motion),
            ),
            (
                "main_diff_reading_context",
                Box::new(crate::editor_diff_extent_reload::reading_context::scenario_main_diff_reading_context),
            ),
            (
                "file_history_reading_context",
                Box::new(crate::editor_diff_extent_reload::reading_context::scenario_file_history_reading_context),
            ),
            (
                "editor_diff_reading_context",
                Box::new(crate::editor_diff_extent_reload::reading_context::scenario_editor_diff_reading_context),
            ),
            (
                "editor_tab_overflow",
                Box::new(crate::editor_tabs::scenario_editor_tab_overflow),
            ),
            (
                "editor_toggle_keys",
                Box::new(crate::editor_tabs::scenario_editor_toggle_keys),
            ),
            (
                "editor_markdown_default_view",
                Box::new(crate::editor_tabs::scenario_editor_markdown_default_view),
            ),
            (
                "editor_find_controls_a11y",
                Box::new(crate::editor_find::scenario_editor_find_controls_a11y),
            ),
            ("pr_threads", Box::new(crate::pr_threads::scenario_pr_threads)),
            (
                "pr_threads_projection_invalidation",
                Box::new(crate::pr_threads_projection::scenario_pr_threads_projection_invalidation),
            ),
            (
                "pr_threads_projection_session_owner",
                Box::new(crate::pr_threads_projection::scenario_pr_threads_projection_session_owner),
            ),
            (
                "pr_threads_mode_anchor_code",
                Box::new(crate::pr_threads_projection::scenario_pr_threads_mode_anchor_code),
            ),
            (
                "pr_threads_mode_anchor_expansion",
                Box::new(crate::pr_threads_projection::scenario_pr_threads_mode_anchor_expansion),
            ),
            (
                "pr_threads_mode_anchor_replacement",
                Box::new(crate::pr_threads_projection::scenario_pr_threads_mode_anchor_replacement),
            ),
            ("review_hunk_full", Box::new(crate::github_text::scenario_review_hunk_full)),
            ("issue_body_zoom", Box::new(crate::github_text::scenario_issue_body_zoom)),
            ("toolbar_no_overlap", Box::new(crate::toolbar_overflow::scenario_toolbar_no_overlap)),
            ("toolbar_pr_sidebar", Box::new(crate::toolbar_pr_sidebar::scenario_toolbar_pr_sidebar)),
            ("toolbar_overflow", Box::new(crate::toolbar_overflow::scenario_toolbar_overflow)),
            (
                "toolbar_overflow_priority",
                Box::new(crate::toolbar_overflow_priority::scenario_toolbar_overflow_priority),
            ),
            ("status_controls", Box::new(crate::status_controls::scenario_status_controls)),
            ("status_chips_semantics", Box::new(crate::status_chips::scenario_status_chips_semantics)),
            ("status_bar_fit", Box::new(crate::status_bar_fit::scenario_status_bar_fit)),
            ("status_bar_busy_height", Box::new(crate::status_bar_fit::scenario_status_bar_busy_height)),
            ("status_bar_feedback_min", Box::new(crate::status_bar_fit::scenario_status_bar_feedback_min)),
            ("status_bar_short_branch", Box::new(crate::status_bar_fit::scenario_status_bar_short_branch)),
            (
                "pr_threads_via_gh",
                Box::new(crate::pr_threads::scenario_pr_threads_via_gh),
            ),
            (
                "pr_suggestion_apply",
                Box::new(crate::pr_suggestion_apply::scenario_pr_suggestion_apply),
            ),
            (
                "operation_strip_abort",
                Box::new(crate::app_conflict::scenario_operation_strip_abort),
            ),
            (
                "operation_strip_stale_detector",
                Box::new(crate::app_conflict::scenario_operation_strip_stale_detector),
            ),
            (
                "conflict_detect_no_revision_laundering",
                Box::new(crate::app_conflict::scenario_conflict_detect_no_revision_laundering),
            ),
            (
                "conflict_background_owner",
                Box::new(crate::app_conflict::scenario_conflict_background_owner),
            ),
            (
                "conflict_revalidates_after_external_abort",
                Box::new(
                    crate::app_conflict::scenario_conflict_revalidates_after_external_abort,
                ),
            ),
            (
                "conflict_deferred_action_owner",
                Box::new(crate::app_conflict::scenario_conflict_deferred_action_owner),
            ),
            (
                "conflict_detect_wrong_owner_is_dropped",
                Box::new(crate::app_conflict::scenario_conflict_detect_wrong_owner_is_dropped),
            ),
            (
                "conflict_detect_stale_clear_is_dropped",
                Box::new(crate::app_conflict::scenario_conflict_detect_stale_clear_is_dropped),
            ),
            (
                "conflict_awaited_detection_stale_settles",
                Box::new(crate::app_conflict::scenario_conflict_awaited_detection_stale_settles),
            ),
            (
                "stash_public_boundary",
                Box::new(crate::app_stash::scenario_stash_public_boundary),
            ),
            (
                "stash_entry_identity",
                Box::new(crate::stash_entry_identity::scenario_stash_entry_identity),
            ),
            (
                "stash_apply_manifest",
                Box::new(crate::stash_apply_manifest::scenario_stash_apply_manifest),
            ),
            (
                "stash_push_plan_accuracy",
                Box::new(crate::app_stash::scenario_stash_push_plan_accuracy),
            ),
            (
                "stash_remote_drop",
                Box::new(crate::app_stash::scenario_remote_stash_drop),
            ),
            (
                "stash_conflict_followup",
                Box::new(crate::app_stash::scenario_stash_conflict_followup),
            ),
            (
                "stash_apply_conflict_preview",
                Box::new(crate::app_stash::scenario_stash_apply_conflict_preview),
            ),
            (
                "stash_conflict_close_reopen",
                Box::new(crate::app_stash::scenario_stash_conflict_close_reopen),
            ),
            (
                "stash_replan_error",
                Box::new(crate::app_stash::scenario_stash_replan_error),
            ),
            (
                "create_branch_input_confirm_ime",
                Box::new(crate::recovery_operations::scenario_create_branch_input_confirm_ime),
            ),
            (
                "create_branch_warnings",
                Box::new(crate::create_branch_warnings::scenario_create_branch_warnings),
            ),
            (
                "create_branch_cancel_reopen",
                Box::new(crate::create_branch_cancel_reopen::scenario_create_branch_cancel_reopen),
            ),
            (
                "create_branch_feedback",
                Box::new(crate::create_branch_feedback::scenario_create_branch_feedback),
            ),
            (
                "create_branch_after_unchanged",
                Box::new(crate::create_branch_feedback::scenario_create_branch_after_unchanged),
            ),
            (
                "stash_push_stacked_preview",
                Box::new(crate::recovery_operations::scenario_stash_push_stacked_preview),
            ),
            (
                "input_confirm_disabled_cards",
                Box::new(crate::recovery_operations::scenario_input_confirm_disabled_cards),
            ),
            (
                "modal_widths_ordered",
                Box::new(crate::recovery_operations::scenario_modal_widths_ordered),
            ),
            (
                "create_branch_replan_error",
                Box::new(crate::recovery_operations::scenario_create_branch_replan_error),
            ),
            (
                "external_stash_conflict_has_no_drop_prompt",
                Box::new(crate::app_stash::scenario_external_stash_conflict_has_no_drop_prompt),
            ),
            (
                "commit_row_layout",
                Box::new(crate::recovery_layout::scenario_commit_row_layout),
            ),
            (
                // #767: the WIP anchor node, asserted against the window's own
                // paint trace. Read-only over four working trees, so every one
                // of them is fingerprinted before and after.
                "commit_row_layout_wip",
                Box::new(|cx| {
                    let (_fixture, repo, wt_a, wt_b) = build_two_worktree_fixture();
                    let before = [
                        repo_fingerprint(&repo),
                        repo_fingerprint(&wt_a),
                        repo_fingerprint(&wt_b),
                    ];
                    crate::recovery_layout::wip::scenario_commit_row_layout_wip(cx, &repo);
                    assert_eq!(
                        before,
                        [
                            repo_fingerprint(&repo),
                            repo_fingerprint(&wt_a),
                            repo_fingerprint(&wt_b)
                        ],
                        "drawing the WIP anchors mutated a working tree"
                    );

                    let (_edge_fixture, repo, shared, detached, orphan) =
                        build_wip_anchor_edge_fixture();
                    let before = [
                        repo_fingerprint(&repo),
                        repo_fingerprint(&shared),
                        repo_fingerprint(&detached),
                        repo_fingerprint(&orphan),
                    ];
                    crate::recovery_layout::wip::scenario_commit_row_layout_wip_edges(
                        cx, &repo, &shared, &detached, &orphan,
                    );
                    assert_eq!(
                        before,
                        [
                            repo_fingerprint(&repo),
                            repo_fingerprint(&shared),
                            repo_fingerprint(&detached),
                            repo_fingerprint(&orphan)
                        ],
                        "drawing the WIP anchors mutated a working tree"
                    );
                }),
            ),
            (
                "editor_history_layout",
                Box::new(|cx| {
                    let fixture = build_fixture();
                    let before = repo_fingerprint(fixture.path());
                    crate::recovery_layout::scenario_editor_history_layout(cx, fixture.path());
                    assert_eq!(before, repo_fingerprint(fixture.path()));
                }),
            ),
            (
                "editor_snapshot_readonly",
                Box::new(crate::editor_snapshot_readonly::scenario),
            ),
            (
                "editor_history_fetch_error",
                Box::new(crate::editor_history_error::scenario),
            ),
            (
                "blame_toggle_a11y",
                Box::new(crate::blame_toggle::blame_toggle_a11y),
            ),
            (
                "blame_dirty_mapping",
                Box::new(crate::blame_identity::dirty_mapping),
            ),
            (
                "blame_full_identity",
                Box::new(crate::blame_identity::full_identity),
            ),
            (
                "blame_crlf_identity",
                Box::new(crate::blame_identity::crlf_identity),
            ),
            (
                "blame_hidden_mapping",
                Box::new(crate::blame_identity::hidden_mapping),
            ),
            (
                "file_history_virtualized",
                Box::new(|cx| {
                    let fixture = build_fixture();
                    let before = repo_fingerprint(fixture.path());
                    crate::recovery_layout::scenario_file_history_virtualized(cx, fixture.path());
                    assert_eq!(before, repo_fingerprint(fixture.path()));
                }),
            ),
            (
                "file_history_open_file",
                Box::new(crate::file_history_open::scenario),
            ),
            ("commit_panel_density", Box::new(crate::commit_panel_density::scenario)),
            ("changes_file_motion", Box::new(crate::changes_motion::scenario)),
            ("changes_inline_pointer_focus", Box::new(crate::file_row_input::inline_pointer_focus)),
            ("inspector_file_motion", Box::new(crate::inspector_motion::scenario)),
            ("inspector_generated_viewport_motion", Box::new(crate::inspector_motion::scenario_generated_viewport)),
            (
                "inspector_directory_accordion_motion",
                Box::new(crate::inspector_motion::scenario_directory_accordion_motion),
            ),
            ("inspector_more_motion", Box::new(crate::file_row_input::inspector_more)),
            ("modal_disclosure_motion", Box::new(crate::modal_motion::scenario_modal_disclosure_motion)),
            (
                "inspector_files_keyboard",
                Box::new(crate::inspector_history_keyboard::inspector),
            ),
            (
                "file_history_focus_return",
                Box::new(crate::inspector_history_keyboard::history),
            ),
            (
                "inspector_generated_focus_retained",
                Box::new(crate::inspector_history_keyboard::generated_focus_retained),
            ),
            (
                "inspector_overflow_keyboard",
                Box::new(crate::inspector_history_keyboard::overflow_keyboard),
            ),
            (
                "header_fit",
                Box::new(crate::recovery_header_fit::scenario_header_fit),
            ),
            (
                "diff_header_ax_unique",
                Box::new(crate::recovery_header_fit::scenario_diff_header_ax_unique),
            ),
            (
                "diff_mode_segment",
                Box::new(crate::recovery_diff_presentation::scenario_diff_mode_segment),
            ),
            (
                "diff_binary_states",
                Box::new(crate::recovery_diff_presentation::scenario_diff_binary_states),
            ),
            (
                "lfs_binary_preview",
                Box::new(crate::recovery_diff_presentation::scenario_lfs_binary_preview),
            ),
            (
                "conflict_image_zoom",
                Box::new(crate::recovery_diff_presentation::scenario_conflict_image_zoom),
            ),
            (
                "commit_panel_viewport",
                Box::new(crate::recovery_commit_panel_viewport::scenario_commit_panel_viewport),
            ),
            ("compact_rail_cmd_f", Box::new(crate::compact_rail::scenario_compact_rail_cmd_f)),
            ("compact_rail_nested_escape", Box::new(crate::compact_rail::escape::scenario_compact_rail_nested_escape)),
            ("compact_pr_menu_escape", Box::new(crate::compact_rail::escape::scenario_compact_pr_menu_escape)),
            ("compact_group_reveal_escape", Box::new(crate::compact_rail::escape::scenario_compact_group_reveal_escape)),
            ("compact_rail_short_window", Box::new(crate::compact_rail::scenario_compact_rail_short_window)),
            ("compact_popover_sections", Box::new(crate::compact_rail::scenario_compact_popover_sections)),
            ("pr_section_pointer", Box::new(crate::compact_rail::section_click::scenario_pr_section_pointer)),
            ("compact_rail_ax", Box::new(crate::compact_rail::scenario_compact_rail_ax)),
            ("compact_pr_popover_scroll", Box::new(crate::compact_rail::scroll::scenario_compact_pr_popover_scroll)),
            (
                "compact_select_changes",
                Box::new(crate::compact_select_changes::scenario_compact_select_changes),
            ),
            (
                "compact_wip_first_frame",
                Box::new(crate::compact_select_changes::scenario_compact_wip_first_frame),
            ),
            (
                "compact_wip_probe_superseded",
                Box::new(crate::compact_select_changes::scenario_compact_wip_probe_superseded),
            ),
            (
                "compact_wip_amend_loading",
                Box::new(crate::compact_select_changes::scenario_compact_wip_amend_loading),
            ),
            (
                "graph_return_reopens_changes",
                Box::new(crate::graph_return_changes::scenario_graph_return_reopens_changes),
            ),
            (
                "graph_return_input_escape",
                Box::new(crate::graph_return_changes::scenario_graph_return_input_escape),
            ),
            (
                "graph_return_bottom_focus",
                Box::new(crate::graph_return_changes::scenario_graph_return_bottom_focus),
            ),
            (
                "compact_layout_rail",
                Box::new(crate::compact_layout::rail::scenario_compact_layout_rail),
            ),
            (
                "compact_layout_changes",
                Box::new(crate::compact_layout::changes::scenario_compact_layout_changes),
            ),
            (
                "compact_changes_initial_file",
                Box::new(crate::compact_layout::initial_file::scenario_compact_changes_initial_file),
            ),
            (
                "compact_changes_deferred_file",
                Box::new(crate::compact_layout::deferred_file::scenario_compact_changes_deferred_file),
            ),
            (
                "compact_inspector_columns",
                Box::new(crate::compact_layout::inspector::scenario_compact_inspector_columns),
            ),
            (
                "compact_inspector_segment_keys",
                Box::new(crate::compact_layout::inspector::scenario_compact_inspector_segment_keys),
            ),
            (
                "commit_panel_bulk_hover",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_commit_panel_bulk_hover),
            ),
            (
                "commit_panel_file_middle_ellipsis",
                Box::new(crate::recovery_commit_panel_file::scenario),
            ),
            (
                "compact_bottom_third",
                Box::new(crate::bottom_panel_divider_drag::scenario_compact_bottom_third),
            ),
            (
                "compact_bottom_presentation_restore",
                Box::new(crate::bottom_panel_divider_drag::scenario_compact_bottom_presentation_restore),
            ),
            (
                "compact_layout_geometry",
                Box::new(crate::compact_layout::scenario_compact_layout_geometry),
            ),
            (
                "compact_layout_geometry_zoom167",
                Box::new(crate::compact_layout::scenario_compact_layout_geometry_zoom167),
            ),
            (
                "compact_layout_geometry_tall",
                Box::new(crate::compact_layout::scenario_compact_layout_geometry_tall),
            ),
            (
                "compact_layout_geometry_tall_zoom167",
                Box::new(crate::compact_layout::scenario_compact_layout_geometry_tall_zoom167),
            ),
            (
                "compact_layout_popup",
                Box::new(crate::compact_layout::popup::scenario_compact_layout_popup),
            ),
            (
                "compact_layout_content_fit",
                Box::new(crate::compact_layout::popup::scenario_compact_layout_content_fit),
            ),
            (
                "compact_commit_preserves_new_draft",
                Box::new(crate::compact_layout::popup::scenario_compact_commit_preserves_new_draft),
            ),
            (
                "queue_history_viewport",
                Box::new(crate::compact_layout::queue_history::scenario_queue_history_viewport),
            ),
            (
                "queue_history_drag_owner",
                Box::new(crate::compact_layout::queue_history::scenario_queue_history_drag_owner),
            ),
            (
                "compact_layout_state",
                Box::new(crate::compact_layout::state::scenario_compact_layout_state),
            ),
            (
                "compact_layout_state_tall",
                Box::new(crate::compact_layout::state::scenario_compact_layout_state_tall),
            ),
            (
                "compact_rem_transitions",
                Box::new(crate::compact_layout::state::scenario_compact_rem_transitions),
            ),
            (
                "compact_layout_lifecycle",
                Box::new(crate::compact_layout::lifecycle::scenario_compact_layout_lifecycle),
            ),
            (
                "commit_panel_i18n",
                Box::new(crate::recovery_commit_panel_i18n::scenario_commit_panel_i18n),
            ),
            (
                "commit_panel_selection_hover",
                Box::new(crate::recovery_commit_panel_polish::scenario_commit_panel_selection_hover),
            ),
            (
                "commit_panel_body_focus",
                Box::new(crate::recovery_commit_panel_polish::scenario_commit_panel_body_focus),
            ),
            (
                "changes_rows_keyboard",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_changes_rows_keyboard),
            ),
            (
                "changes_focus_follows_selection",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_changes_focus_follows_selection),
            ),
            (
                "changes_discard_keyboard",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_changes_discard_keyboard),
            ),
            (
                "changes_file_menu_viewport",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_changes_file_menu_viewport),
            ),
            (
                "commit_panel_segment_keys",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_commit_panel_segment_keys),
            ),
            (
                "commit_panel_footer_tab_order",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_commit_panel_footer_tab_order),
            ),
            (
                "smart_commit_footer_provider",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_smart_commit_footer_provider),
            ),
            (
                "smart_generation_commit_boundary",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_smart_generation_commit_boundary),
            ),
            (
                "smart_generation_replaced_panel",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_smart_generation_replaced_panel),
            ),
            (
                "commit_draft_save_failure",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_commit_draft_save_failure),
            ),
            (
                "smart_replace_undo",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_smart_replace_undo),
            ),
            (
                "commit_panel_coauthor_keys",
                Box::new(crate::recovery_commit_panel_keyboard::scenario_commit_panel_coauthor_keys),
            ),
            (
                "toolbar_count_scale",
                Box::new(crate::toolbar_count::scenario_toolbar_count_scale),
            ),
            (
                "file_history_wip_changes",
                Box::new(|cx| {
                    let fixture = build_fixture();
                    // #813: an unstaged edit so File History leads with a WIP row.
                    std::fs::write(
                        fixture.path().join("README.md"),
                        "# fixture\nsecond line\nthird line\n",
                    )
                    .unwrap();
                    let before = repo_fingerprint(fixture.path());
                    crate::recovery_layout::scenario_file_history_wip_changes(cx, fixture.path());
                    assert_eq!(before, repo_fingerprint(fixture.path()));
                }),
            ),
            (
                "footer_status_line",
                Box::new(|cx| {
                    let fixture = build_fixture();
                    let before = repo_fingerprint(fixture.path());
                    crate::recovery_layout::scenario_footer_status_line(cx, fixture.path());
                    assert_eq!(before, repo_fingerprint(fixture.path()));
                }),
            ),
            (
                "read_owner_switch",
                Box::new(crate::read_owner::scenario_read_owner_switch),
            ),
            (
                "read_owner_ordering",
                Box::new(crate::read_owner::scenario_read_owner_ordering),
            ),
            (
                "load_more_stale_reads",
                Box::new(crate::read_owner::scenario_load_more_stale_reads),
            ),
            (
                "tab_ui_state_ownership",
                Box::new(crate::tab_ui_state::scenario_tab_ui_state_ownership),
            ),
            (
                "tab_ui_state_background_reload",
                Box::new(crate::tab_ui_state::scenario_tab_ui_state_background_reload),
            ),
            (
                "pr_mode_retained_on_activation",
                Box::new(crate::tab_ui_state::scenario_pr_mode_retained_on_activation),
            ),
            (
                "pr_open_enters_before_ref_fetch",
                Box::new(crate::tab_ui_state::scenario_pr_open_enters_before_ref_fetch),
            ),
            (
                "tab_ui_state_rejects_detached_writer",
                Box::new(crate::tab_ui_state::scenario_tab_ui_state_rejects_detached_writer),
            ),
            (
                "retained_pane_resources",
                Box::new(crate::pane_resources::scenario_retained_pane_resources),
            ),
            (
                "commit_stage_deferred_owner",
                Box::new(crate::pane_resources::scenario_commit_stage_deferred_owner),
            ),
            (
                "close_tab_editor_dirty_owner",
                Box::new(crate::pane_resources::scenario_close_tab_editor_dirty_owner),
            ),
            (
                "host_dirty_guard",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard),
            ),
            (
                "host_dirty_guard_save",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard_save),
            ),
            (
                "host_dirty_guard_relaunch",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard_relaunch),
            ),
            (
                "host_dirty_guard_unknown_recovery",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard_unknown_recovery),
            ),
            (
                "host_dirty_guard_clean_restart",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard_clean_restart),
            ),
            (
                "host_dirty_guard_installed_restart",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard_installed_restart),
            ),
            (
                "host_dirty_guard_save_waiting",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard_save_waiting),
            ),
            (
                "host_dirty_guard_save_unwind",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard_save_unwind),
            ),
            (
                "host_dirty_guard_close_then_quit",
                Box::new(crate::host_dirty_guard::scenario_host_dirty_guard_close_then_quit),
            ),
            (
                "host_guard_released_recovery_running",
                Box::new(crate::host_guard_hold_reason::scenario_host_guard_released_recovery_running),
            ),
            (
                "host_guard_async_update_preserves_modal",
                Box::new(crate::host_guard_async_restart::scenario_host_guard_async_update_preserves_modal),
            ),
            (
                "welcome_startup_renders",
                Box::new(crate::pane_resources::scenario_welcome_startup_renders),
            ),
            (
                "close_last_tab_welcome_renders",
                Box::new(crate::pane_resources::scenario_close_last_tab_welcome_renders),
            ),
            (
                "remote_connect_keeps_dirty_editor",
                Box::new(crate::pane_resources::scenario_remote_connect_keeps_dirty_editor),
            ),
            (
                "smart_generation_close_drops_panel",
                Box::new(crate::pane_resources::scenario_smart_generation_close_drops_panel),
            ),
            (
                "commit_close_drops_panel",
                Box::new(crate::pane_resources::scenario_commit_close_drops_panel),
            ),
            (
                "commit_panel_refuses_during_activation",
                Box::new(crate::pane_resources::scenario_commit_panel_refuses_during_activation),
            ),
            (
                "manual_reload_releases_revalidation",
                Box::new(crate::pane_resources::scenario_manual_reload_releases_revalidation),
            ),
            (
                "conflict_pane_survives_activation",
                Box::new(crate::app_conflict::scenario_conflict_pane_survives_activation),
            ),
            (
                "commit_panel_revalidates_on_activation",
                Box::new(crate::pane_resources::scenario_commit_panel_revalidates_on_activation),
            ),
            (
                "read_cache_revalidates_on_activation",
                Box::new(
                    crate::cache_history_owner::scenario_read_cache_revalidates_on_activation,
                ),
            ),
            (
                "operation_history_session_and_stale_ref",
                Box::new(
                    crate::cache_history_owner::scenario_operation_history_session_and_stale_ref,
                ),
            ),
            (
                "background_operation_history_owner",
                Box::new(
                    crate::cache_history_owner::scenario_background_operation_history_owner,
                ),
            ),
            (
                "welcome_drops_root_main_diff",
                Box::new(crate::cache_history_owner::scenario_welcome_drops_root_main_diff),
            ),
            (
                "remote_refresh_departed_owner",
                Box::new(crate::remote_refresh_owner::scenario_remote_refresh_departed_owner),
            ),
            (
                "remote_connect_owner",
                Box::new(crate::remote_refresh_owner::scenario_remote_connect_owner),
            ),
            (
                "remote_refresh_newest_request",
                Box::new(crate::remote_refresh_owner::scenario_remote_refresh_newest_request),
            ),
            (
                "fetch_auth_failure_toast",
                Box::new(crate::fetch_auth_error::scenario_fetch_auth_failure_toast),
            ),
            (
                "fetch_same_owner_piggybacks",
                Box::new(crate::fetch_owner::scenario_fetch_same_owner_piggybacks),
            ),
            (
                "fetch_different_owner_does_not_piggyback",
                Box::new(crate::fetch_owner::scenario_fetch_different_owner_does_not_piggyback),
            ),
            (
                "fetch_detach_retains_flight_and_isolates_reopen",
                Box::new(crate::fetch_owner::scenario_fetch_detach_retains_flight_and_isolates_reopen),
            ),
            (
                "fetch_owner_display_isolated",
                Box::new(crate::fetch_owner::scenario_fetch_owner_display_isolated),
            ),
            (
                "fetch_panicked_worker_reconciles",
                Box::new(crate::fetch_owner::scenario_fetch_panicked_worker_reconciles),
            ),
            (
                "fetch_previous_visit_is_not_presented",
                Box::new(crate::fetch_owner::scenario_fetch_previous_visit_is_not_presented),
            ),
            (
                "fetch_old_visit_drops_pull_waiter",
                Box::new(crate::fetch_owner::scenario_fetch_old_visit_drops_pull_waiter),
            ),
            (
                "fetch_new_visit_waiter_sees_old_flight_failure",
                Box::new(crate::fetch_owner::scenario_fetch_new_visit_waiter_sees_old_flight_failure),
            ),
            (
                "fetch_new_visit_waiter_success_notifies",
                Box::new(crate::fetch_owner::scenario_fetch_new_visit_waiter_success_notifies),
            ),
            (
                "auto_fetch_old_visit_logs_contract",
                Box::new(crate::fetch_owner::scenario_auto_fetch_old_visit_logs_contract),
            ),
            (
                "remote_branch_fetch_panic",
                Box::new(crate::guard_writer_panic::scenario_remote_branch_fetch_panic),
            ),
            (
                "snapshot_write_panic",
                Box::new(crate::guard_writer_panic::scenario_snapshot_write_panic),
            ),
            (
                "snapshot_write_draws_while_busy",
                Box::new(crate::guard_writer_panic::scenario_snapshot_write_draws_while_busy),
            ),
            (
                "snapshot_failed_after_departure",
                Box::new(crate::guard_writer_panic::scenario_snapshot_failed_after_departure),
            ),
            (
                "conflict_merge_continue_panic",
                Box::new(crate::app_conflict::scenario_conflict_merge_continue_panic),
            ),
            (
                "conflict_confirm_continue_panic",
                Box::new(crate::app_conflict::scenario_conflict_confirm_continue_panic),
            ),
            (
                "conflict_confirm_dismissed_still_reloads",
                Box::new(crate::app_conflict::scenario_conflict_confirm_dismissed_still_reloads),
            ),
            (
                "conflict_merge_failure_contract",
                Box::new(crate::app_conflict::scenario_conflict_merge_failure_contract),
            ),
            (
                "conflict_skip_panic",
                Box::new(crate::app_conflict::scenario_conflict_skip_panic),
            ),
            (
                "conflict_skip_step_race",
                Box::new(crate::app_conflict::scenario_conflict_skip_step_race),
            ),
            (
                "stash_continue_panic",
                Box::new(crate::app_stash::scenario_stash_continue_panic),
            ),
            (
                "stash_continue_after_tab_switch",
                Box::new(crate::app_stash::scenario_stash_continue_after_tab_switch),
            ),
            (
                "remote_branch_fetch_failed_after_departure",
                Box::new(crate::guard_writer_panic::scenario_remote_branch_fetch_failed_after_departure),
            ),
            (
                "remote_branch_fetch_success_after_departure",
                Box::new(crate::guard_writer_panic::scenario_remote_branch_fetch_success_after_departure),
            ),
            (
                "pr_ref_fetch_restarts_after_revisit",
                Box::new(crate::guard_writer_panic::scenario_pr_ref_fetch_restarts_after_revisit),
            ),
            (
                "pr_ref_fetch_panic",
                Box::new(crate::guard_writer_panic::scenario_pr_ref_fetch_panic),
            ),
            (
                "editor_save_panic",
                Box::new(crate::guard_writer_panic::scenario_editor_save_panic),
            ),
            (
                "editor_save_panic_after_close",
                Box::new(crate::guard_writer_panic::scenario_editor_save_panic_after_close),
            ),
            (
                "file_menu_freezes_path",
                Box::new(crate::file_menu_owner::scenario_file_menu_freezes_path),
            ),
            (
                "discard_literal_backslash",
                Box::new(crate::file_menu_owner::scenario_discard_literal_backslash),
            ),
            (
                "discard_unsafe_selection",
                Box::new(crate::file_menu_owner::scenario_discard_unsafe_selection),
            ),
            (
                "file_menu_rejects_stale_owner",
                Box::new(crate::file_menu_owner::scenario_file_menu_rejects_stale_owner),
            ),
            (
                "file_menu_focus_after_open_repository",
                Box::new(crate::file_menu_owner::scenario_file_menu_focus_after_open_repository),
            ),
            (
                "file_menu_unstage_literal",
                Box::new(crate::file_menu_owner::scenario_file_menu_unstage_literal),
            ),
            (
                "pr_peek_visible_table",
                Box::new(crate::pr_peek_visible::scenario_pr_peek_visible_table),
            ),
            (
                "pr_peek_visible_edges",
                Box::new(crate::pr_peek_visible::scenario_pr_peek_visible_edges),
            ),
            (
                "pr_peek_preserves_tabs_drafts",
                Box::new(crate::pr_peek_visible::scenario_pr_peek_preserves_tabs_drafts),
            ),
            (
                "pr_peek_dirty_guard",
                Box::new(crate::pr_peek_visible::scenario_pr_peek_dirty_guard),
            ),
            (
                "pr_peek_read_failure_context",
                Box::new(crate::pr_peek_visible::scenario_pr_peek_read_failure_context),
            ),
            (
                "github_evidence_restores",
                Box::new(crate::github_evidence_owner::scenario_github_evidence_restores),
            ),
            (
                "github_evidence_background_owner",
                Box::new(crate::github_evidence_owner::scenario_github_evidence_background_owner),
            ),
            (
                "github_evidence_detached_owner",
                Box::new(crate::github_evidence_owner::scenario_github_evidence_detached_owner),
            ),
            (
                "cleanup_evidence_background_owner",
                Box::new(crate::cleanup_evidence_owner::scenario_cleanup_evidence_background_owner),
            ),
            (
                "cleanup_evidence_superseded",
                Box::new(crate::cleanup_evidence_owner::scenario_cleanup_evidence_superseded),
            ),
            (
                "cleanup_evidence_read_revision",
                Box::new(crate::cleanup_evidence_owner::scenario_cleanup_evidence_read_revision),
            ),
            (
                "squash_evidence_read_revision",
                Box::new(crate::squash_evidence_owner::scenario_squash_evidence_read_revision),
            ),
            (
                "cleanup_evidence_publish_generation",
                Box::new(
                    crate::cleanup_publish_owner::scenario_cleanup_evidence_publish_generation,
                ),
            ),
            (
                "squash_evidence_publish_generation",
                Box::new(
                    crate::squash_publish_owner::scenario_squash_evidence_publish_generation,
                ),
            ),
            (
                "squash_graph_routing",
                Box::new(crate::squash_graph_routing::scenario_squash_graph_routing),
            ),
            (
                "conflict_detector_owner_guard",
                Box::new(crate::conflict_evidence_owner::scenario_conflict_detector_owner_guard),
            ),
            (
                "ecosystem_evidence_background_owner",
                Box::new(crate::ecosystem_evidence_owner::scenario_ecosystem_evidence_background_owner),
            ),
            (
                "ecosystem_evidence_superseded",
                Box::new(crate::ecosystem_evidence_owner::scenario_ecosystem_evidence_superseded),
            ),
            (
                "ecosystem_evidence_detached_samepath",
                Box::new(crate::ecosystem_evidence_owner::scenario_ecosystem_evidence_detached_samepath),
            ),
            (
                "folder_drop_open",
                Box::new(crate::folder_drop::scenario_folder_drop_open),
            ),
            (
                "folder_drop_exit",
                Box::new(crate::folder_drop::scenario_folder_drop_exit),
            ),
            (
                "folder_drop_occluded",
                Box::new(crate::folder_drop::scenario_folder_drop_occluded),
            ),
            (
                "folder_urls_existing_root",
                Box::new(crate::folder_urls::scenario_folder_urls_existing_root),
            ),
            (
                "folder_urls_without_root",
                Box::new(crate::folder_urls::scenario_folder_urls_without_root),
            ),
            (
                "folder_urls_settings_only",
                Box::new(crate::folder_urls::scenario_folder_urls_settings_only),
            ),
            (
                "remote_source_merge_into",
                Box::new(crate::recovery_operations::scenario_remote_source_merge_into),
            ),
            (
                "branch_drag_self_target",
                Box::new(crate::dnd_self::scenario_branch_drag_self_target),
            ),
            (
                "branch_drag_detached_target",
                Box::new(crate::dnd_self::scenario_branch_drag_detached_target),
            ),
            (
                "cross_worktree_merge",
                Box::new(crate::worktree_graph::scenario_cross_worktree_merge),
            ),
            (
                "stage_external_filter",
                Box::new(crate::recovery_operations::scenario_stage_external_filter),
            ),
            (
                "stage_failure_notice",
                Box::new(crate::recovery_operations::scenario_stage_failure_notice),
            ),
            (
                "user_text_native",
                Box::new(crate::user_text::scenario_user_text_native),
            ),
            (
                "batch_staging_refresh",
                Box::new(crate::batch_staging_refresh::scenario_batch_staging_refresh),
            ),
            (
                "hunk_staging",
                Box::new(crate::hunk_staging::scenario_hunk_staging),
            ),
            (
                "diff_hunk_buttons",
                Box::new(crate::diff_keyboard::scenario_diff_hunk_buttons),
            ),
            (
                "diff_keyboard_selection",
                Box::new(crate::diff_keyboard::scenario_diff_keyboard_selection),
            ),
            (
                "split_diff_keyboard_order",
                Box::new(crate::diff_keyboard::scenario_split_diff_keyboard_order),
            ),
            (
                "diff_selection_revision",
                Box::new(crate::diff_selection_revision::scenario_diff_selection_revision),
            ),
            (
                "diff_split_unicode_badge",
                Box::new(crate::diff_split_unicode_badge::scenario_diff_split_unicode_badge),
            ),
            (
                "diff_line_ending_annotation",
                Box::new(crate::diff_line_ending_annotation::scenario_diff_line_ending_annotation),
            ),
            (
                "hunk_staging_content_identity",
                Box::new(crate::hunk_staging::scenario_hunk_staging_content_identity),
            ),
            (
                "wip_diffstat_stage_order",
                Box::new(crate::hunk_staging::scenario_wip_diffstat_stage_order),
            ),
            (
                "wip_diffstat_watcher_order",
                Box::new(crate::hunk_staging::scenario_wip_diffstat_watcher_order),
            ),
            (
                "issue_failure_notice_survives_tab_switch",
                Box::new(
                    crate::issue_write_owner::scenario_issue_failure_notice_survives_tab_switch,
                ),
            ),
            (
                "platform_menu_scroll",
                Box::new(crate::platform_menu_scroll::scenario_platform_menu_scroll),
            ),
            (
                "platform_menu_over_modal",
                Box::new(crate::platform_menu_scroll::scenario_platform_menu_over_modal),
            ),
            (
                "filter_menu_over_modal",
                Box::new(crate::platform_menu_scroll::scenario_filter_menu_over_modal),
            ),
            (
                "filter_menu_then_settings",
                Box::new(crate::platform_menu_scroll::scenario_filter_menu_then_settings),
            ),
            (
                "coauthor_menu_under_conflict",
                Box::new(crate::platform_menu_scroll::scenario_coauthor_menu_under_conflict),
            ),
            (
                "platform_menu_modal_settings",
                Box::new(crate::platform_menu_scroll::scenario_platform_menu_modal_settings),
            ),
            (
                "platform_menu_settings_focus",
                Box::new(crate::platform_menu_focus::scenario_platform_menu_settings_focus),
            ),
            (
                "platform_menu_modal_settings_order",
                Box::new(
                    crate::platform_menu_focus::scenario_platform_menu_modal_settings_order,
                ),
            ),
            (
                "platform_menu_settings_command_order",
                Box::new(
                    crate::platform_menu_focus::scenario_platform_menu_settings_command_order,
                ),
            ),
            (
                "platform_menu_repository_navigation",
                Box::new(
                    crate::platform_menu_focus::scenario_platform_menu_repository_navigation,
                ),
            ),
            (
                "platform_menu_close_tab_command_navigation",
                Box::new(
                    crate::platform_menu_focus::scenario_platform_menu_close_tab_command_navigation,
                ),
            ),
            (
                "branch_picker_screen_departure",
                Box::new(crate::platform_menu_focus::scenario_branch_picker_screen_departure),
            ),
            (
                "platform_menu_palette_focus_return",
                Box::new(crate::platform_menu_focus::scenario_platform_menu_palette_focus_return),
            ),
            (
                "field_picker_owner",
                Box::new(crate::field_picker_owner::scenario_field_picker_owner),
            ),
            (
                "issue_create_fields",
                Box::new(crate::issue_create_fields::scenario_issue_create_fields),
            ),
            (
                "ghe_viewer_login",
                Box::new(crate::ghe_viewer_login::scenario_ghe_viewer_login),
            ),
            (
                "ghe_viewer_login_closed_only",
                Box::new(crate::ghe_viewer_login::scenario_ghe_viewer_login_closed_only),
            ),
            (
                "workspace_mode_toolbar",
                Box::new(crate::workspace_mode_toolbar::scenario_workspace_mode_toolbar),
            ),
            (
                "markdown_preparation_edges",
                Box::new(
                    crate::workspace_mode_toolbar::markdown_preparation_edges::scenario_markdown_preparation_edges,
                ),
            ),
            (
                "issue_conversation_offscreen_copy",
                Box::new(crate::issue_conversation_selection::scenario_issue_conversation_offscreen_copy),
            ),
            (
                "issue_conversation_giant_copy",
                Box::new(crate::issue_conversation_selection::scenario_issue_conversation_giant_copy),
            ),
            (
                "issue_conversation_accepted_identity",
                Box::new(crate::issue_conversation_selection::scenario_issue_conversation_accepted_identity),
            ),
            (
                "issue_conversation_refresh_anchor",
                Box::new(crate::issue_conversation_selection::scenario_issue_conversation_refresh_anchor),
            ),
            (
                "issue_conversation_geometry",
                Box::new(crate::issue_conversation_selection::scenario_issue_conversation_geometry),
            ),
            (
                "issue_conversation_edge_drag",
                Box::new(crate::issue_conversation_selection::scenario_issue_conversation_edge_drag),
            ),
            (
                "issue_conversation_delayed_rejected",
                Box::new(crate::issue_conversation_lifecycle::scenario_issue_conversation_delayed_rejected),
            ),
            (
                "issue_conversation_return_pending",
                Box::new(crate::issue_conversation_lifecycle::scenario_issue_conversation_return_pending),
            ),
            (
                "issue_conversation_return_failed",
                Box::new(crate::issue_conversation_lifecycle::scenario_issue_conversation_return_failed),
            ),
            (
                "issue_conversation_two_owners",
                Box::new(crate::issue_conversation_lifecycle::scenario_issue_conversation_two_owners),
            ),
            (
                "issue_conversation_copy_priority",
                Box::new(crate::issue_conversation_lifecycle::scenario_issue_conversation_copy_priority),
            ),
            (
                "issue_conversation_enabled_input_copy",
                Box::new(crate::issue_enabled_input_copy::scenario_issue_conversation_enabled_input_copy),
            ),
            (
                "issue_conversation_offscreen_select_all",
                Box::new(crate::issue_offscreen_select_all::scenario_issue_conversation_offscreen_select_all),
            ),
            (
                "toolbar_a11y_disabled",
                Box::new(crate::workspace_mode_toolbar::scenario_toolbar_a11y_disabled),
            ),
            (
                "toolbar_keyboard_reasons",
                Box::new(crate::toolbar_keyboard::scenario_toolbar_keyboard_reasons),
            ),
            (
                "issues_pagination",
                Box::new(crate::issues_pagination::scenario_issues_pagination),
            ),
            (
                "pr_pagination",
                Box::new(crate::pr_pagination::scenario_pr_pagination),
            ),
            (
                "pr_pagination_races",
                Box::new(crate::pr_pagination::scenario_pr_pagination_races),
            ),
            (
                "pr_pagination_closed_tab_retention",
                Box::new(crate::pr_pagination::scenario_pr_pagination_closed_tab_retention),
            ),
            (
                "pr_pagination_all_tab_retention",
                Box::new(crate::pr_pagination::scenario_pr_pagination_all_tab_retention),
            ),
            (
                "pr_pagination_pending_closed_tab_return",
                Box::new(crate::pr_pagination::scenario_pr_pagination_pending_closed_tab_return),
            ),
            (
                "pr_pagination_pending_all_tab_return",
                Box::new(crate::pr_pagination::scenario_pr_pagination_pending_all_tab_return),
            ),
            (
                "pr_paging_survives_periodic_tick",
                Box::new(crate::pr_pagination::scenario_pr_paging_survives_periodic_tick),
            ),
            (
                "pr_periodic_pending_sidebar_and_other_collections",
                Box::new(
                    crate::pr_pagination::scenario_pr_periodic_pending_sidebar_and_other_collections,
                ),
            ),
            (
                "pr_single_page_periodic_scroll_retention",
                Box::new(crate::pr_pagination::scenario_pr_single_page_periodic_scroll_retention),
            ),
            ("bottom_panel", Box::new(scenario_bottom_panel)),
            (
                "activity_granularity_keys",
                Box::new(crate::activity_a11y::scenario_activity_granularity_keys),
            ),
            (
                "activity_bucket_a11y",
                Box::new(crate::activity_a11y::scenario_activity_bucket_a11y),
            ),
            (
                "activity_equal_series_encoding",
                Box::new(crate::activity_a11y::scenario_activity_equal_series_encoding),
            ),
            (
                "activity_single_bucket_encoding",
                Box::new(crate::activity_a11y::scenario_activity_single_bucket_encoding),
            ),
            (
                "activity_bucket_hit",
                Box::new(crate::activity_bucket_hit::scenario_activity_bucket_hit),
            ),
            (
                "activity_bucket_hit_light_100",
                Box::new(crate::activity_bucket_hit::scenario_activity_bucket_hit_light_100),
            ),
            (
                "activity_bucket_hit_dark_100",
                Box::new(crate::activity_bucket_hit::scenario_activity_bucket_hit_dark_100),
            ),
            (
                "activity_bucket_hit_dark_167",
                Box::new(crate::activity_bucket_hit::scenario_activity_bucket_hit_dark_167),
            ),
            (
                "bottom_panel_tab_hover",
                Box::new(crate::recovery_bottom_panel::scenario_bottom_panel_tab_hover),
            ),
            (
                "bottom_panel_nested",
                Box::new(crate::recovery_bottom_panel::scenario_bottom_panel_nested),
            ),
            (
                "bottom_panel_motion",
                Box::new(crate::bottom_panel_motion::scenario_bottom_panel_motion),
            ),
            (
                "bottom_panel_divider_drag",
                Box::new(crate::bottom_panel_divider_drag::scenario_bottom_panel_divider_drag),
            ),
            (
                "sidebar_motion",
                Box::new(crate::side_panel_motion::scenario_sidebar_motion),
            ),
            (
                "side_panel_motion",
                Box::new(crate::side_panel_motion::scenario_side_panel_motion),
            ),
            (
                "right_selection_motion",
                Box::new(crate::side_panel_motion::scenario_right_selection_motion),
            ),
            (
                "pane_budget_fit",
                Box::new(crate::pane_budget::scenario_pane_budget_fit),
            ),
            (
                "pane_budget_worktree_anchor",
                Box::new(crate::pane_budget::scenario_pane_budget_worktree_anchor),
            ),
            (
                "pane_budget_group_anchor",
                Box::new(crate::pane_budget::scenario_pane_budget_group_anchor),
            ),
            (
                "graph_column_divider_hidden",
                Box::new(crate::graph_column_divider::scenario_graph_column_divider_hidden),
            ),
            (
                "graph_columns_fit",
                Box::new(crate::graph_column_divider::scenario_graph_columns_fit),
            ),
            (
                "graph_columns_compressed_drag",
                Box::new(crate::graph_column_divider::scenario_graph_columns_compressed_drag),
            ),
            (
                "graph_columns_inspector_fit",
                Box::new(crate::graph_column_divider::scenario_graph_columns_inspector_fit),
            ),
            ("graph_copy", Box::new(scenario_graph_copy)),
            ("graph_copy_selected_branch", Box::new(crate::branch_copy::scenario_graph_copy_selected_branch)),
            (
                "graph_pointer_focus",
                Box::new(crate::graph_focus::scenario_graph_pointer_focus),
            ),
            (
                "graph_copy_foreground",
                Box::new(crate::graph_focus::scenario_graph_copy_foreground),
            ),
            (
                "diff_survives_reload",
                Box::new(scenario_diff_survives_reload),
            ),
            (
                "compare_survives_reload",
                Box::new(scenario_compare_survives_reload),
            ),
            (
                "wip_diff_survives_reload",
                Box::new(scenario_wip_diff_survives_reload),
            ),
            (
                "commit_panel_survives_reload",
                Box::new(scenario_commit_panel_survives_reload),
            ),
            ("oplog_expand_copy", Box::new(scenario_oplog_expand_copy)),
            ("oplog_row_typography", Box::new(crate::density_rest::oplog)),
            ("issues_row_secondary_text", Box::new(crate::density_rest::issues)),
            (
                "oplog_keyboard_single",
                Box::new(crate::recovery_oplog_keyboard::scenario_oplog_keyboard_single),
            ),
            (
                "oplog_keyboard_many",
                Box::new(crate::recovery_oplog_keyboard::scenario_oplog_keyboard_many),
            ),
            (
                "oplog_keyboard_refresh_tail",
                Box::new(crate::recovery_oplog_keyboard::scenario_oplog_keyboard_refresh_tail),
            ),
            (
                "oplog_keyboard_refresh_top",
                Box::new(crate::recovery_oplog_keyboard::scenario_oplog_keyboard_refresh_top),
            ),
            (
                "oplog_keyboard_copy_focus",
                Box::new(crate::recovery_oplog_keyboard::scenario_oplog_keyboard_copy_focus),
            ),
            (
                "oplog_detail_select_copy",
                Box::new(crate::recovery_oplog_select::scenario_oplog_detail_select_copy),
            ),
            (
                "oplog_recovery_recorded",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_recovery_recorded),
            ),
            (
                "oplog_recovery_failed_omitted",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_recovery_failed_omitted),
            ),
            (
                "oplog_recovery_old_not_recorded",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_recovery_old_not_recorded),
            ),
            (
                "oplog_actor_reflog",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_actor_reflog),
            ),
            (
                "oplog_row_167_result_visible",
                Box::new(crate::recovery_oplog_layout::scenario_oplog_row_167_result_visible),
            ),
            (
                "oplog_revert_eligibility",
                Box::new(crate::recovery_oplog_layout::scenario_oplog_revert_eligibility),
            ),
            (
                "oplog_restore_card",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_restore_card),
            ),
            (
                "oplog_restore_keyboard",
                Box::new(crate::recovery_oplog_restore_keys::scenario_oplog_restore_keyboard),
            ),
            (
                "oplog_restore_body_keys",
                Box::new(crate::recovery_oplog_restore_keys::scenario_oplog_restore_body_keys),
            ),
            (
                "oplog_restore_body_wheel",
                Box::new(crate::recovery_oplog_restore_keys::scenario_oplog_restore_body_wheel),
            ),
            (
                "oplog_restore_pane_wheel",
                Box::new(crate::recovery_oplog_restore_keys::scenario_oplog_restore_pane_wheel),
            ),
            (
                "oplog_restore_guarded_rows",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_restore_guarded_rows),
            ),
            (
                "oplog_restore_tag_preview",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_restore_tag_preview),
            ),
            (
                "oplog_restore_preview_review",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_restore_preview_review),
            ),
            (
                "oplog_restore_across_merge",
                Box::new(crate::recovery_oplog_panel::scenario_oplog_restore_across_merge),
            ),
            ("create_snapshot", Box::new(scenario_create_snapshot)),
            ("theme_switch", Box::new(scenario_theme_switch)),
            ("agent_provenance", Box::new(scenario_agent_provenance)),
            ("wip_head_connector", Box::new(scenario_wip_head_connector)),
            (
                "graph_worktree_open",
                Box::new(crate::worktree_graph::scenario_graph_worktree_open),
            ),
            ("worktree_inspection", Box::new(crate::worktree_inspection::scenario)),
            (
                "worktree_wip_inline",
                Box::new(scenario_worktree_wip_inline),
            ),
            (
                "worktree_panel_commit",
                Box::new(scenario_worktree_panel_commit),
            ),
            (
                "worktree_panel_discard_recording_failure",
                Box::new(scenario_worktree_panel_discard_recording_failure),
            ),
            (
                "worktree_panel_amend_discard",
                Box::new(scenario_worktree_panel_amend_discard),
            ),
            (
                "oplog_detail_draw",
                Box::new(crate::perf_oplog_detail::scenario_expanded_detail_draw),
            ),
            (
                "inspector_metadata_a11y",
                Box::new(crate::inspector_metadata::scenario_inspector_metadata_a11y),
            ),
            (
                "inspector_message_links",
                Box::new(crate::inspector_message_links::scenario),
            ),
            (
                "finder_scale_rest",
                Box::new(crate::finder_scale_rest::scenario),
            ),
            (
                "inspector_scale",
                Box::new(crate::inspector_scale::scenario),
            ),
            (
                "inspector_file_row_hit",
                Box::new(crate::inspector_file_rows::scenario),
            ),
            (
                "inspector_directory_accordion",
                Box::new(crate::inspector_file_rows::scenario_directory_accordion),
            ),
            (
                "compact_inspector_directory_accordion",
                Box::new(crate::compact_layout::scenario_compact_inspector_directory_accordion),
            ),
            (
                "graph_subject_identity",
                Box::new(crate::text_caps::scenario_graph_subject_identity),
            ),
            (
                "inspector_full_message",
                Box::new(crate::text_caps::scenario_inspector_full_message),
            ),
            (
                "text_caps_warm_render",
                Box::new(crate::text_caps::scenario_text_caps_warm_render),
            ),
            (
                "inspector_derived",
                Box::new(crate::perf_inspector_derived::scenario_inspector_derived),
            ),
            (
                "inspector_generated_lockfile_plain",
                Box::new(
                    crate::perf_inspector_derived::scenario_inspector_generated_lockfile_plain,
                ),
            ),
        ];
        let inventory = match crate::inventory::prepare(filters.as_deref(), exact.as_deref()) {
            Ok(requested) => requested,
            Err(error) => {
                eprintln!("[inventory] ERROR: {error}");
                return 1;
            }
        };
        crate::inventory::register(
            &mut scenarios,
            inventory,
            filters.as_deref(),
            exact.as_deref(),
        );
        // #516: KAGI_GUI_E2E_KEEP_GOING=1 re-runs this runner once per
        // scenario instead (`keep_going`), before anything here is set up.
        if exact.is_none()
            && (inventory || std::env::var("KAGI_GUI_E2E_KEEP_GOING").as_deref() == Ok("1"))
        {
            // #967 review: the filter stays required. KEEP_GOING without it
            // would run every scenario, each for up to the timeout — the
            // unscoped run that once took macOS down (#549).
            let Some(filters) = filters.as_deref() else {
                eprintln!(
                    "[gui-e2e] ERROR: KAGI_GUI_E2E_KEEP_GOING needs KAGI_GUI_E2E_ONLY=<scenario substrings>"
                );
                return 1;
            };
            return keep_going(&scenarios, Some(filters));
        }

        // #516: one directory owns everything the run writes. `TMPDIR` points
        // into it for the whole run (`tempfile`, git and shells use it), so
        // whatever a scenario leaves behind there is the scenario's own — not
        // another process's on a shared machine.
        let run_root = tempfile::Builder::new()
            .prefix("kagi-gui-e2e-")
            .tempdir()
            .expect("runner root");
        let run_tmp = run_root.path().join("tmp");
        std::fs::create_dir(&run_tmp).expect("runner TMPDIR");
        std::env::set_var("TMPDIR", &run_tmp);
        // Redirect settings.json to a throwaway dir so scenarios that touch
        // settings (graph_copy_target, theme via set_active) never read or clobber
        // the developer's real `~/.kagi/settings.json` (ADR-0091 flat-string file).
        let log_dir = run_root.path().join("log");
        std::fs::create_dir(&log_dir).expect("settings dir");
        std::env::set_var("KAGI_LOG_DIR", &log_dir);
        // #516: and `HOME`. The product (the editor's trash, `gh`'s working
        // directory, terminal fonts), the `git` and `gh` it runs and the shells it starts all
        // read the developer's dotfiles under it: identity, aliases, hooks,
        // credential helpers, rc files, history. Here they find an empty home
        // with only a fixed identity, the fixtures' own.
        let home = run_root.path().join("home");
        std::fs::create_dir(&home).expect("runner HOME");
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = poc\n\temail = poc@example.com\n[init]\n\tdefaultBranch = main\n",
        )
        .expect("runner git identity");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        // #963 review: `HOME` alone is not enough. An inherited `GIT_*`
        // still reaches every `git` the product runs: `GIT_CONFIG_GLOBAL` /
        // `GIT_CONFIG_SYSTEM` naming the developer's config by absolute path,
        // `GIT_CONFIG_COUNT` / `GIT_CONFIG_PARAMETERS` injecting keys,
        // `GIT_DIR` / `GIT_WORK_TREE` from a hook or `git bisect run`. Drop
        // them all, as `support/git_fixture.rs` does for fixture commands, then
        // pin only what the run needs: the global config is the run's own
        // `.gitconfig`, there is no system config, and nothing prompts.
        // The same for `gh`: `GH_CONFIG_DIR` outranks `$XDG_CONFIG_HOME/gh`,
        // and `GH_TOKEN` / `GITHUB_TOKEN` (and their Enterprise forms)
        // outrank its stored login, so a scenario whose seam missed would
        // talk to GitHub as the developer (`gh help environment`). With them
        // gone, `gh` reads the run's empty home.
        let inherited: Vec<_> = std::env::vars_os()
            .map(|(key, _)| key)
            .filter(|key| {
                let key = key.to_string_lossy();
                key.starts_with("GIT_")
                    || key.starts_with("GH_")
                    || key == "GITHUB_TOKEN"
                    || key == "GITHUB_ENTERPRISE_TOKEN"
            })
            .collect();
        for key in inherited {
            std::env::remove_var(key);
        }
        std::env::set_var("GIT_CONFIG_GLOBAL", home.join(".gitconfig"));
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
        std::env::set_var("GIT_TERMINAL_PROMPT", "0");
        // The fixtures assume a new repository starts on `main` — a bare
        // `origin` they push `main` to and then clone. That came from Apple
        // Git's vendor config (`/Library/Developer/CommandLineTools/usr/share/
        // git-core/gitconfig`: `init.defaultBranch = main`, next to
        // `credential.helper = osxkeychain`), which `GIT_CONFIG_NOSYSTEM`
        // rightly drops. Pin the one value back: in the run's `.gitconfig`
        // above for the product's `git` (which clears `GIT_CONFIG_COUNT`),
        // and as command-scope config for the fixture `git` helpers, which
        // set `GIT_CONFIG_GLOBAL=/dev/null` and so never read that file.
        std::env::set_var("GIT_CONFIG_COUNT", "1");
        std::env::set_var("GIT_CONFIG_KEY_0", "init.defaultBranch");
        std::env::set_var("GIT_CONFIG_VALUE_0", "main");
        // #516: a failing scenario leaves evidence in target/gui-e2e/<name>/.
        crate::gui_evidence::install();

        // Shared context: real Mac platform + bundled assets, one-time app init
        // (fonts, gpui_component, theme sync, the cmd-j / cmd-c bindings).
        // Effective bindings are cached at startup, exactly as in the product.
        let _shortcut_bindings = matches!(
            exact.as_deref(),
            Some("palette_shortcut_glyphs" | "help_shortcuts_glyphs")
        )
        .then(crate::palette_polish::install_shortcut_bindings);
        theme::init_active();
        let mut cx = VisualTestAppContext::with_asset_source(e2e::platform(), e2e::asset_source());
        let native_pool = NativeAutoreleasePool::new();
        cx.update(e2e::init_app);
        let mut executed = 0;
        for (name, scenario) in &mut scenarios {
            if selected(name, filters.as_deref(), exact.as_deref()) {
                set_current_scenario(name);
                crate::gui_evidence::begin(name);
                // #516 slice 3: shared state is compared inside the same
                // boundary, so a leak fails like an assertion, with evidence.
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let before = crate::gui_isolation::snapshot();
                    scenario(&mut cx);
                    crate::gui_isolation::check(name, &before);
                }));
                match run {
                    Ok(()) => crate::gui_evidence::passed(),
                    Err(panic) => {
                        crate::gui_evidence::finish();
                        std::panic::resume_unwind(panic);
                    }
                }
                executed += 1;
            } else if exact.is_none() {
                eprintln!("[gui-e2e] SKIP {} (filtered)", name);
            }
        }
        if executed == 0 {
            eprintln!(
                "[gui-e2e] ERROR: KAGI_GUI_E2E_ONLY={filters:?} matched no enabled scenarios"
            );
            return 1;
        }
        cx.run_until_parked();
        drain_native_events();
        drop(native_pool);
        cx.update(|_| {});
        cx.run_until_parked();

        if exact.is_some() {
            // A re-executed child: the KEEP_GOING parent reports the run.
        } else if filters.is_none() {
            eprintln!("[gui-e2e] PASS all scenarios");
        } else {
            eprintln!("[gui-e2e] PASS filtered scenarios");
        }
        0
    }

    pub(crate) type Scenario = (&'static str, Box<dyn FnMut(&mut VisualTestAppContext)>);

    /// Whether `name` runs: only the one a re-executed child was given
    /// (`KAGI_GUI_E2E_EXACT`), else every match of `KAGI_GUI_E2E_ONLY`.
    fn selected(name: &str, filters: Option<&[String]>, exact: Option<&str>) -> bool {
        match exact {
            Some(exact) => name == exact,
            None => {
                filters.is_none_or(|filters| filters.iter().any(|filter| name.contains(filter)))
            }
        }
    }

    /// How long one re-executed scenario may run before `keep_going` kills it
    /// (`KAGI_GUI_E2E_TIMEOUT_SECS` overrides it), so a hung scenario fails
    /// on its own instead of stopping the run.
    const SCENARIO_TIMEOUT: Duration = Duration::from_secs(600);

    /// How one re-executed scenario ended.
    enum ChildOutcome {
        Passed,
        Exited(std::process::ExitStatus),
        TimedOut(Duration),
        /// The parent got SIGINT / SIGTERM; the child was stopped like a
        /// timed-out one.
        Interrupted,
        NotStarted(std::io::Error),
    }

    /// Set by [`on_interrupt`] when this KEEP_GOING parent gets SIGINT or
    /// SIGTERM.
    static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    extern "C" fn on_interrupt(_signal: std::ffi::c_int) {
        // Async-signal-safe: one atomic store, nothing else.
        INTERRUPTED.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    extern "C" {
        fn signal(signum: std::ffi::c_int, handler: extern "C" fn(std::ffi::c_int)) -> usize;
    }

    /// Each child runner leads a process group of its own, so the terminal's
    /// Ctrl+C reaches only this parent (#967 review). Catch it — and SIGTERM —
    /// so the running child, its group and its recorded descendants are
    /// stopped and reaped, and its `TMPDIR` removed, before the run ends.
    fn catch_interrupts() {
        const SIGINT: std::ffi::c_int = 2;
        const SIGTERM: std::ffi::c_int = 15;
        // SAFETY: installs a handler that only stores to an atomic.
        unsafe {
            signal(SIGINT, on_interrupt);
            signal(SIGTERM, on_interrupt);
        }
    }

    fn interrupted() -> bool {
        INTERRUPTED.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// `KAGI_GUI_E2E_KEEP_GOING=1` (#516): run each selected scenario in its
    /// own re-executed runner (`KAGI_GUI_E2E_EXACT=<name>`, one at a time), so
    /// a failure — a panic, a crash that takes the process down, or a hang
    /// past the timeout — fails that scenario and the rest still run. A
    /// panicked GPUI context is never reused. Each child sets up its own run
    /// directory, `HOME`, settings and evidence, exactly as a single filtered
    /// run does, under a `TMPDIR` this process removes after it. Prints one
    /// PASS / FAIL line per scenario; returns 1 if any failed.
    fn keep_going(scenarios: &[Scenario], filters: Option<&[String]>) -> i32 {
        let names: Vec<&str> = scenarios
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| selected(name, filters, None))
            .collect();
        if names.is_empty() {
            eprintln!(
                "[gui-e2e] ERROR: KAGI_GUI_E2E_ONLY={filters:?} matched no enabled scenarios"
            );
            return 1;
        }
        let timeout = std::env::var("KAGI_GUI_E2E_TIMEOUT_SECS")
            .ok()
            .and_then(|secs| secs.parse().ok())
            .map_or(SCENARIO_TIMEOUT, Duration::from_secs);
        let runner = std::env::current_exe().expect("the runner's own executable");
        catch_interrupts();
        let mut results = Vec::with_capacity(names.len());
        for (at, name) in names.iter().enumerate() {
            if interrupted() {
                break;
            }
            eprintln!("[gui-e2e] KEEP_GOING {}/{} {name}", at + 1, names.len());
            // A directory that exists afterwards is this child's, not an
            // earlier run's: a child that dies before `begin` leaves none.
            let evidence = crate::gui_evidence::evidence_dir(name);
            let _ = std::fs::remove_dir_all(&evidence);
            let (outcome, cleanup) = run_child(&runner, name, timeout);
            let ended = match &outcome {
                ChildOutcome::Passed => None,
                ChildOutcome::Exited(status) => Some(status.to_string()),
                ChildOutcome::TimedOut(after) => {
                    Some(format!("timeout after {}s (killed)", after.as_secs()))
                }
                ChildOutcome::Interrupted => Some("interrupted (killed)".to_string()),
                ChildOutcome::NotStarted(error) => {
                    Some(format!("the runner did not start: {error}"))
                }
            };
            // A process the scenario started that survived the kill would
            // run next to the following scenarios: that fails the scenario,
            // and its pids are named so it can be found. So does a cleanup
            // that cannot tell what is left (see `Tracked::settled`).
            let left = (!cleanup.is_empty()).then(|| cleanup.join("; "));
            let line = match (ended, left) {
                (Some(ended), Some(left)) => Some(format!("{ended}; {left}")),
                (ended, left) => ended.or(left),
            };
            if let Some(reason) = &line {
                crate::gui_evidence::exit_record(name, reason);
                crate::inventory::record_child_failure(name, reason);
            }
            results.push((*name, line, evidence));
        }
        let failed: Vec<&str> = results
            .iter()
            .filter(|(_, line, _)| line.is_some())
            .map(|(name, _, _)| *name)
            .collect();
        let passed = results.len() - failed.len();
        eprintln!(
            "[gui-e2e] KEEP_GOING summary: {passed} passed, {} failed of {}",
            failed.len(),
            names.len()
        );
        if interrupted() {
            let not_run = &names[results.len()..];
            eprintln!(
                "[gui-e2e] KEEP_GOING interrupted; not run: {}",
                if not_run.is_empty() {
                    "none".to_string()
                } else {
                    not_run.join(", ")
                }
            );
        }
        for (name, line, evidence) in &results {
            match line {
                None => eprintln!("[gui-e2e]   PASS {name}"),
                Some(reason) => {
                    let shown = std::path::absolute(evidence).unwrap_or(evidence.clone());
                    eprintln!(
                        "[gui-e2e]   FAIL {name}: {reason} — evidence {}",
                        shown.display()
                    );
                }
            }
        }
        if interrupted() {
            return 130;
        }
        if !failed.is_empty() {
            eprintln!(
                "[gui-e2e] FAIL {} of {} scenarios: {}",
                failed.len(),
                names.len(),
                failed.join(", ")
            );
            return 1;
        }
        if filters.is_none() {
            eprintln!("[gui-e2e] PASS all scenarios");
        } else {
            eprintln!("[gui-e2e] PASS filtered scenarios");
        }
        0
    }

    /// Run `name` alone in a fresh runner process and wait for it, killing it
    /// past `timeout`. Its `TMPDIR` is a directory of its own, removed once it
    /// has ended, so even a killed child leaves no fixture behind.
    ///
    /// Nothing the scenario started may outlive it next to the next one
    /// (#967 review). The child leads a process group of its own, but the
    /// product puts some of what it starts outside that group: the terminal's
    /// PTY shell runs `setsid`, and `kagi_git::proc` gives every `git` / `ssh`
    /// a group of its own. So while the child runs, every process descended
    /// from it is recorded (pid and start time, re-read every
    /// [`TREE_POLL`]); when it ends — by itself or killed at the timeout —
    /// each recorded process still running with the same start time is
    /// killed. Its group is killed only at the timeout, while its leader is
    /// still unreaped: after the reap the pgid is a number the OS may hand to
    /// someone else (the rule in `kagi_git::proc::group`). (A group of its
    /// own also means the terminal's Ctrl+C reaches only this parent, which
    /// then stops the child as at a timeout: see [`catch_interrupts`].)
    fn run_child(runner: &Path, name: &str, timeout: Duration) -> (ChildOutcome, Vec<String>) {
        use std::os::unix::process::CommandExt as _;

        let tmp = match tempfile::Builder::new()
            .prefix("kagi-gui-e2e-child-")
            .tempdir()
        {
            Ok(tmp) => tmp,
            Err(error) => return (ChildOutcome::NotStarted(error), Vec::new()),
        };
        let mut child = match std::process::Command::new(runner)
            .env("KAGI_GUI_E2E_EXACT", name)
            .env_remove("KAGI_GUI_E2E_KEEP_GOING")
            .env("TMPDIR", tmp.path())
            .process_group(0)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => return (ChildOutcome::NotStarted(error), Vec::new()),
        };
        let root = child.id();
        let started = Instant::now();
        let mut tree = Tracked::default();
        let mut polled = started;
        let outcome = loop {
            if polled.elapsed() >= TREE_POLL {
                tree.record(root);
                polled = Instant::now();
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break ChildOutcome::Passed,
                Ok(Some(status)) => break ChildOutcome::Exited(status),
                Ok(None) if started.elapsed() >= timeout || interrupted() => {
                    tree.stop(&mut child);
                    if interrupted() {
                        break ChildOutcome::Interrupted;
                    }
                    break ChildOutcome::TimedOut(timeout);
                }
                Ok(None) => std::thread::sleep(TREE_POLL),
                Err(error) => {
                    tree.stop(&mut child);
                    break ChildOutcome::NotStarted(error);
                }
            }
        };
        // The runner is reaped; what it left running (now orphans, reaped by
        // launchd once killed) ends here, by identity rather than by group.
        tree.kill();
        (outcome, tree.settled())
    }

    /// The processes a scenario's runner started: each pid with the
    /// microsecond it started ([`proc_identity`]), so a pid the OS has
    /// handed to someone else is never signalled. `ps` finds the tree; the
    /// kernel says who each pid is. What could not be read is remembered and
    /// fails the scenario rather than reporting that nothing is left
    /// (#967 review).
    #[derive(Default)]
    struct Tracked {
        seen: std::collections::HashMap<u32, proc_identity::Started>,
        /// Pids in the tree whose identity the kernel would not give, at
        /// recording or at kill time: never signalled.
        unknown: std::collections::BTreeSet<u32>,
        /// The first failure to read the process table.
        table: Option<String>,
        /// Signals that could not be sent.
        failed: Vec<String>,
    }

    impl Tracked {
        /// Record every process now descended from `root`.
        fn record(&mut self, root: u32) {
            match descendants(root) {
                Ok(found) => {
                    for pid in found {
                        match proc_identity::started(pid) {
                            // A pid seen before under another start time was
                            // reused: the earlier process is gone.
                            Ok(Some(at)) => {
                                self.seen.insert(pid, at);
                            }
                            // Gone between `ps` and the kernel read.
                            Ok(None) => {}
                            Err(_) => {
                                self.unknown.insert(pid);
                            }
                        }
                    }
                }
                Err(error) => {
                    self.table.get_or_insert(error);
                }
            }
        }

        /// The recorded processes that are still the same process. Asks the
        /// kernel per pid, not `ps`, so it works when nothing can be spawned.
        fn still_running(&mut self) -> Vec<u32> {
            let mut alive = Vec::new();
            for (&pid, &at) in &self.seen {
                match proc_identity::identity(pid, at) {
                    proc_identity::Identity::Same => alive.push(pid),
                    proc_identity::Identity::Gone => {}
                    proc_identity::Identity::Unknown(_) => {
                        self.unknown.insert(pid);
                    }
                }
            }
            alive
        }

        /// SIGKILL each recorded process still running.
        fn kill(&mut self) {
            for pid in self.still_running() {
                if let Err(error) = proc_identity::sigkill(pid as i32) {
                    self.failed.push(error);
                }
            }
        }

        /// Stop a runner still running at its timeout or an interrupt: read
        /// the tree while it is alive (once killed, its children move to
        /// launchd and cannot be traced back), kill the descendants, then
        /// the runner's group while its leader is unreaped. The reap waits
        /// only for a runner a signal reached.
        fn stop(&mut self, child: &mut std::process::Child) {
            self.record(child.id());
            self.kill();
            let group = proc_identity::sigkill(-(child.id() as i32));
            let reached = match group {
                Ok(()) => true,
                Err(error) => {
                    self.failed.push(error);
                    proc_identity::sigkill(child.id() as i32)
                        .map_err(|error| self.failed.push(error))
                        .is_ok()
                }
            };
            if reached {
                let _ = child.wait();
            } else {
                self.failed
                    .push(format!("runner pid {} not stopped", child.id()));
            }
        }

        /// What is wrong after the cleanup, once a just-killed process has
        /// had a bounded moment to go (2 s; a process in its exit path is
        /// not "left running"). Empty: the scenario left nothing behind.
        fn settled(mut self) -> Vec<String> {
            let mut left = self.still_running();
            for _ in 0..40 {
                if left.is_empty() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
                left = self.still_running();
            }
            let pids = |pids: &mut dyn Iterator<Item = &u32>| {
                pids.map(u32::to_string).collect::<Vec<_>>().join(", ")
            };
            let mut problems = Vec::new();
            if !left.is_empty() {
                left.sort_unstable();
                problems.push(format!(
                    "left running after the kill: pid {}",
                    pids(&mut left.iter())
                ));
            }
            if let Some(error) = &self.table {
                problems.push(format!(
                    "process table unavailable ({error}); what is left is unknown"
                ));
            }
            if !self.unknown.is_empty() {
                problems.push(format!(
                    "identity unknown, not killed: pid {}",
                    pids(&mut self.unknown.iter())
                ));
            }
            problems.extend(self.failed);
            problems
        }
    }

    /// How often `run_child` re-reads the process tree under a scenario.
    ///
    /// The known limit (#967 review): a process started — and moved to a
    /// session of its own — after the last read and before the runner ends
    /// by itself is not recorded, and survives. macOS has no supported way
    /// to follow every fork (`NOTE_TRACK` for `EVFILT_PROC` is "no longer
    /// supported as of 10.5", `sys/event.h`), so the window is narrowed to
    /// one poll interval rather than closed; see `docs/decisions.md`. A
    /// timeout reads the tree once more before it kills, so a hung scenario
    /// has no such window.
    const TREE_POLL: Duration = Duration::from_millis(50);

    /// Every live process `(pid, parent)`. A `ps` that fails (it may not
    /// start when the scenario has used up the process or FD limit) is tried
    /// 3 times, 50 ms apart, before it is an error.
    fn process_table() -> Result<Vec<(u32, u32)>, String> {
        let mut result = read_process_table();
        for _ in 0..2 {
            if result.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            result = read_process_table();
        }
        result
    }

    /// One `ps`. Zombies (state `Z`) are not live: they have exited and only
    /// wait to be reaped. A `ps` that cannot start, fails, prints a line it
    /// cannot parse or prints no process at all (it always lists itself) is
    /// an error, never an empty table.
    fn read_process_table() -> Result<Vec<(u32, u32)>, String> {
        let out = Command::new("/bin/ps")
            .args(["-axo", "pid=,ppid=,stat="])
            .output()
            .map_err(|error| format!("ps did not run: {error}"))?;
        if !out.status.success() {
            return Err(format!(
                "ps failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let mut table = Vec::new();
        let mut listed = false;
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let [pid, ppid, stat] = fields.as_slice() else {
                return Err(format!("ps printed a line it should not: {line:?}"));
            };
            let (Ok(pid), Ok(ppid)) = (pid.parse(), ppid.parse()) else {
                return Err(format!("ps printed a line it should not: {line:?}"));
            };
            listed = true;
            if !stat.starts_with('Z') {
                table.push((pid, ppid));
            }
        }
        if !listed {
            return Err("ps listed no process".to_string());
        }
        Ok(table)
    }

    /// The processes descended from `root` (not `root` itself), whatever
    /// group or session they are in.
    fn descendants(root: u32) -> Result<std::collections::BTreeSet<u32>, String> {
        let table = process_table()?;
        let mut found = std::collections::BTreeSet::new();
        let mut frontier = vec![root];
        while let Some(parent) = frontier.pop() {
            for &(pid, ppid) in &table {
                if ppid == parent && found.insert(pid) {
                    frontier.push(pid);
                }
            }
        }
        Ok(found)
    }

    fn assert_center_bottom_panel(window: AnyWindowHandle) -> Bounds<Pixels> {
        let bounds = |name| {
            e2e::control_bounds(window.window_id(), name)
                .unwrap_or_else(|| panic!("{name} not painted"))
        };
        let panel = bounds("bottom-panel");
        let sidebar = bounds("worktree-sidebar");
        let center = bounds("commit-list-viewport");
        let right = bounds("divider-panel");
        let near = |a: f32, b: f32| (a - b).abs() <= 2.;
        let panel_left = f32::from(panel.origin.x);
        let panel_right = panel_left + f32::from(panel.size.width);
        let panel_bottom = f32::from(panel.origin.y + panel.size.height);
        let sidebar_right = f32::from(sidebar.origin.x + sidebar.size.width);
        let sidebar_bottom = f32::from(sidebar.origin.y + sidebar.size.height);
        let right_bottom = f32::from(right.origin.y + right.size.height);
        assert!(
            panel_left >= sidebar_right,
            "panel overlaps sidebar: {panel:?} {sidebar:?}"
        );
        assert!(
            near(panel_left, f32::from(center.origin.x)),
            "panel not aligned with center"
        );
        assert!(
            near(panel_right, f32::from(right.origin.x)),
            "panel extends under inspector"
        );
        assert!(
            near(panel_bottom, sidebar_bottom),
            "sidebar stops above panel bottom"
        );
        assert!(
            near(panel_bottom, right_bottom),
            "inspector stops above panel bottom"
        );
        assert!(
            f32::from(center.origin.y + center.size.height) <= f32::from(panel.origin.y) + 2.,
            "center content overlaps panel"
        );
        panel
    }

    /// Native layout and interactions: the bottom panel occupies the center
    /// column only; Cmd-J, the action, and resizing keep that geometry.
    fn scenario_bottom_panel(cx: &mut VisualTestAppContext) {
        let _saved = crate::gui_isolation::SavedKeys::keep(&["bottom_panel_height"]);
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let before_fp = repo_fingerprint(&repo_path);
        let (kagi, win) = mount(cx, &repo_path);
        kagi.update(cx, |app, cx| {
            app.ui_mut().expect("active session").selected = Some(0);
            cx.notify();
        });
        cx.run_until_parked();
        let redraw = |cx: &mut VisualTestAppContext| {
            cx.update_window(win, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .expect("draw panel layout");
        };
        redraw(cx);
        let initial = cx.read(|app| kagi.read(app).bottom_panel_open);
        assert!(initial, "the default Terminal panel must be visible");
        let initial_panel = assert_center_bottom_panel(win);
        capture_screenshot_best_effort(cx, win, "before");

        cx.simulate_keystrokes(win, "cmd-j"); // keyboard → ToggleBottomPanel
        let after_key = cx.read(|app| kagi.read(app).bottom_panel_open);
        assert_eq!(
            after_key, !initial,
            "cmd-j keystroke should toggle bottom_panel_open ({initial} -> {})",
            !initial
        );
        redraw(cx);
        e2e::clear_control_bounds(win.window_id(), "bottom-panel");
        redraw(cx);
        assert!(
            e2e::control_bounds(win.window_id(), "bottom-panel").is_none(),
            "closed panel should not be drawn"
        );

        cx.dispatch_action(win, ToggleBottomPanel); // registered action path
        let after_action = cx.read(|app| kagi.read(app).bottom_panel_open);
        assert_eq!(
            after_action, initial,
            "ToggleBottomPanel action should restore bottom_panel_open to {initial}"
        );
        redraw(cx);
        let panel = assert_center_bottom_panel(win);
        let pointer = point(panel.origin.x + px(100.), panel.origin.y + px(2.));
        cx.simulate_mouse_move(win, pointer, None, Modifiers::none());
        cx.simulate_mouse_down(win, pointer, MouseButton::Left, Modifiers::none());
        let first = point(pointer.x, pointer.y - px(12.));
        cx.simulate_mouse_move(win, first, MouseButton::Left, Modifiers::none());
        let moved = point(pointer.x, pointer.y - px(48.));
        cx.simulate_mouse_move(win, moved, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(win, moved, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        redraw(cx);
        let resized = assert_center_bottom_panel(win);
        assert!(
            f32::from(resized.size.height) > f32::from(initial_panel.size.height) + 30.,
            "dragging the center-only panel divider should enlarge it"
        );

        capture_screenshot_best_effort(cx, win, "after");
        assert_eq!(
            before_fp,
            repo_fingerprint(&repo_path),
            "repo mutated during a read-only scenario"
        );
        unmount(cx, kagi, win);
        eprintln!("[gui-e2e] PASS bottom_panel initial={initial}");
    }

    /// ADR-0170 graph Cmd+C: with a graph row selected and root focus, the
    /// `CopyDiffSelection` action (no diff selection → Graph gets first refusal)
    /// writes the row's full SHA (`graph_copy_target=hash`) or its local branch
    /// name (`…=branch`) to the clipboard. Asserts the clipboard text each way.
    fn scenario_graph_copy(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let (kagi, win) = mount(cx, &repo_path);

        // Select the HEAD row; capture its full SHA + the branch name the
        // production copy path would yield (via the real `graph_copy_value`, so
        // the badge-label decoration — `"main ✓"` etc. — is handled identically).
        let (full_sha, branch) = kagi.update(cx, |app, cx| {
            app.ui_mut().expect("active session").selected = Some(0);
            cx.notify();
            let row = &app.view().rows[0];
            let full_sha = row.id.0.clone();
            let branch = commit_list::graph_copy_value(&row.badges, &full_sha, CopyTarget::Branch);
            (full_sha, branch)
        });
        assert_ne!(
            branch, full_sha,
            "HEAD row should resolve to a local branch (not fall back to the SHA)"
        );
        cx.run_until_parked();

        // hash mode → clipboard == full SHA
        // Through the settings store, which keeps every other key; the
        // scenario puts the key back as it found it (#516).
        let before = kagi::ui::settings::read_setting("graph_copy_target");
        kagi::ui::settings::write_setting("graph_copy_target", Some("hash"));
        cx.dispatch_action(win, CopyDiffSelection);
        let copied = cx.read_from_clipboard().and_then(|i| i.text());
        assert_eq!(
            copied.as_deref(),
            Some(full_sha.as_str()),
            "graph Cmd+C (hash) should copy the full SHA"
        );

        // branch mode → clipboard == local branch name
        kagi::ui::settings::write_setting("graph_copy_target", Some("branch"));
        cx.dispatch_action(win, CopyDiffSelection);
        let copied = cx.read_from_clipboard().and_then(|i| i.text());
        assert_eq!(
            copied.as_deref(),
            Some(branch.as_str()),
            "graph Cmd+C (branch) should copy the local branch name"
        );
        kagi::ui::settings::write_setting("graph_copy_target", before.as_deref());
        unmount(cx, kagi, win);
        eprintln!(
            "[gui-e2e] PASS graph_copy hash={} branch={branch}",
            &full_sha[..8]
        );
    }

    /// An external refresh (auto-fetch, FS watcher) must NOT close the
    /// full-width diff of a *commit* and dump the reader back on the graph —
    /// a commit's diff is immutable, so there is nothing to invalidate. Opens
    /// the HEAD commit's first file, commits again from outside so every graph
    /// row shifts down one, reloads, and asserts the pane is still up and
    /// re-anchored to the same commit's NEW row index.
    fn scenario_diff_survives_reload(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let (kagi, win) = mount(cx, &repo_path);

        // Select the HEAD row and let the render trigger load its changed
        // files (`open_main_diff_commit` reads them out of the diff cache).
        let sha = kagi.update(cx, |app, cx| {
            app.select(0);
            cx.notify();
            app.view().rows[0].id.0.clone()
        });
        cx.run_until_parked();
        kagi.update(cx, |app, cx| app.open_main_diff_commit(0, cx));
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).ui().main_diff.is_some()),
            "the HEAD commit's first file should open in the main diff pane"
        );

        // A commit lands from outside: the row the diff was opened from is no
        // longer row 0.
        std::fs::write(repo_path.join("README.md"), "# fixture\nthird line\n").unwrap();
        git(&repo_path, &["commit", "-q", "-am", "third commit"]);
        kagi.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();

        cx.read(|app| {
            let app_ref = kagi.read(app);
            assert_eq!(
                app_ref.view().rows[1].id.0,
                sha,
                "the diffed commit should have shifted to row 1"
            );
            let pane = app_ref
                .ui()
                .main_diff
                .as_ref()
                .expect("external reload must not close a commit's diff");
            match pane.read(app).view.source {
                MainDiffSource::Commit {
                    row_index,
                    file_index,
                    ..
                } => assert_eq!(
                    (row_index, file_index),
                    (1, 0),
                    "the pane should be re-anchored to the commit's new row"
                ),
                _ => panic!("expected a Commit-sourced diff"),
            }
        });

        unmount(cx, kagi, win);
        eprintln!("[gui-e2e] PASS diff_survives_reload");
    }

    /// The same rule for the Compare pane and the diff opened out of it: a
    /// reload used to close both. Compare HEAD against the working tree, open a
    /// file, land an unrelated commit from outside, and assert the compare list
    /// and the diff are both still there — re-read, not merely kept.
    fn scenario_compare_survives_reload(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        // A working-tree change to compare against, plus a second file for the
        // external commit to touch.
        std::fs::write(repo_path.join("README.md"), "# fixture\nlocal edit\n").unwrap();
        let (kagi, win) = mount(cx, &repo_path);

        let head = kagi.update(cx, |app, cx| {
            let head = app.view().rows[0].id.clone();
            app.open_compare_with_working_tree(head.clone(), Some(cx));
            head
        });
        cx.run_until_parked();
        kagi.update(cx, |app, cx| app.open_main_diff_compare(0, cx));
        cx.run_until_parked();
        assert!(
            cx.read(|app| {
                let state = kagi.read(app);
                state.ui().compare_view.is_some() && state.ui().main_diff.is_some()
            }),
            "compare + its file diff should be open"
        );

        std::fs::write(repo_path.join("other.txt"), "unrelated\n").unwrap();
        git(&repo_path, &["add", "other.txt"]);
        git(&repo_path, &["commit", "-q", "-m", "unrelated commit"]);
        kagi.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();

        cx.read(|app| {
            let app_ref = kagi.read(app);
            let compare = app_ref
                .ui()
                .compare_view
                .as_ref()
                .expect("external reload must not close the compare pane");
            assert_eq!(
                compare.read(app).view().base,
                head,
                "the compare should still be against the same base commit"
            );
            let pane = app_ref
                .ui()
                .main_diff
                .as_ref()
                .expect("external reload must not close a compare file diff");
            assert!(
                matches!(pane.read(app).view.source, MainDiffSource::Compare { .. }),
                "the pane should still be compare-sourced"
            );
        });

        unmount(cx, kagi, win);
        eprintln!("[gui-e2e] PASS compare_survives_reload");
    }

    /// And for a Commit Panel file's diff, which has to be re-read (its content
    /// really can change): an unrelated external commit leaves it open with
    /// fresh content, and committing the file itself — nothing left to show —
    /// closes it rather than freezing a diff the repository no longer has.
    fn scenario_wip_diff_survives_reload(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        std::fs::write(repo_path.join("README.md"), "# fixture\nwip edit\n").unwrap();
        let (kagi, win) = mount(cx, &repo_path);

        let panel_path = repo_path.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_local_panel_no_inputs(app, panel_path, cx);
        });
        cx.run_until_parked();
        kagi.update(cx, |app, cx| {
            app.open_main_diff_wip(CommitPanelFileRef::Unstaged { index: 0 }, cx)
        });
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).ui().main_diff.is_some()),
            "the unstaged file's diff should be open"
        );

        // An unrelated commit lands: the edit is still uncommitted, so the diff
        // stays up.
        std::fs::write(repo_path.join("other.txt"), "unrelated\n").unwrap();
        git(&repo_path, &["add", "other.txt"]);
        git(&repo_path, &["commit", "-q", "-m", "unrelated commit"]);
        kagi.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();
        cx.read(|app| {
            let pane = kagi
                .read(app)
                .ui()
                .main_diff
                .as_ref()
                .expect("external reload must not close an unstaged file's diff");
            assert!(
                matches!(pane.read(app).view.source, MainDiffSource::Unstaged { .. }),
                "the pane should still be unstaged-sourced"
            );
        });

        // Now the edit itself is committed from outside: there is no unstaged
        // diff left, so the pane closes.
        git(&repo_path, &["commit", "-q", "-am", "commit the wip edit"]);
        kagi.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).ui().main_diff.is_none()),
            "a file with nothing left to diff should close the pane"
        );

        unmount(cx, kagi, win);
        eprintln!("[gui-e2e] PASS wip_diff_survives_reload");
    }

    /// The Commit Panel's display condition is "this working tree has something
    /// to list" — the same condition its WIP row appears under. A reload used
    /// to drop the panel unconditionally, which closed the one the user was
    /// typing into on every auto-fetch. An unrelated external commit leaves the
    /// panel (and the typed message) up; committing everything leaves nothing
    /// to list, so it closes.
    fn scenario_commit_panel_survives_reload(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        std::fs::write(repo_path.join("README.md"), "# fixture\nwip edit\n").unwrap();
        let (kagi, win) = mount(cx, &repo_path);

        let panel_path = repo_path.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_local_panel_no_inputs(app, panel_path, cx);
        });
        cx.run_until_parked();
        // No `InputState`s in the runner (see `open_local_panel_no_inputs`), so
        // the message is the `state.commit_msg` fallback.
        let panel_entity = cx.read(|app| kagi.read(app).ui().commit_panel.clone().expect("panel"));
        cx.update(|app| {
            panel_entity.update(app, |v, _| v.state.commit_msg = "half typed".to_string())
        });
        assert!(
            cx.read(|app| kagi.read(app).ui().commit_panel_open),
            "the commit panel should be open"
        );

        // An unrelated commit lands from outside: README.md is still dirty, so
        // there is still something to list.
        std::fs::write(repo_path.join("other.txt"), "unrelated\n").unwrap();
        git(&repo_path, &["add", "other.txt"]);
        git(&repo_path, &["commit", "-q", "-m", "unrelated commit"]);
        kagi.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();
        cx.read(|app| {
            let app_ref = kagi.read(app);
            assert!(
                app_ref.ui().commit_panel_open,
                "an external commit must not close the panel while the tree is dirty"
            );
            let panel = app_ref.ui().commit_panel.as_ref().expect("panel");
            assert_eq!(
                panel.read(app).state.commit_msg,
                "half typed",
                "the message being typed must survive the reload"
            );
        });

        // Everything is committed: nothing left to list, so the panel closes.
        git(&repo_path, &["commit", "-q", "-am", "commit the wip edit"]);
        kagi.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();
        assert!(
            cx.read(|app| {
                let state = kagi.read(app);
                !state.ui().commit_panel_open && state.ui().commit_panel.is_none()
            }),
            "a clean working tree leaves the panel nothing to list"
        );

        unmount(cx, kagi, win);
        eprintln!("[gui-e2e] PASS commit_panel_survives_reload");
    }

    /// Issue #468: the Operation Log row list is variable-height
    /// (`gpui::list` + `ListState`), not `uniform_list`.
    ///
    /// Seeds two entries — row 0 carries a 200-char `error:` — opens the
    /// Operation Log tab, and reads the rows' real laid-out bounds back out of
    /// the panel's `ListState` (`bounds_for_item`, window coordinates).
    /// Expanding row 0 must (a) grow row 0 past its collapsed summary height
    /// and (b) push row 1's y-origin DOWN by that growth, leaving no overlap.
    /// Under the old `uniform_list` neither held: every row was laid out at the
    /// first row's height, so the detail block painted over the row below.
    /// Then drives `OpLogPanel::copy_entry` (the row copy button's handler) and
    /// asserts the clipboard carries the whole entry, 200-char error included —
    /// the part the truncated summary row never shows.
    fn scenario_oplog_expand_copy(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let before_fp = repo_fingerprint(&repo_path);
        let (kagi, win) = mount(cx, &repo_path);

        let long_error = "e".repeat(200);
        kagi.update(cx, |app, cx| {
            app.bottom_panel_open = true;
            app.bottom_tab = BottomTab::OperationLog;
            // Oldest first — `push` puts the newest at the front, so the
            // long-error entry ends up as row 0.
            e2e::push_failed_op(app, "fetch", "short".to_string(), cx);
            e2e::push_failed_op(app, "checkout", long_error.clone(), cx);
            cx.notify();
        });
        cx.run_until_parked();

        let panel = cx
            .read(|app| kagi.read(app).op_log.clone())
            .expect("op_log entity");
        let row_bounds = |cx: &VisualTestAppContext, ix: usize| {
            cx.read(|app| panel.read(app).scroll_handle().bounds_for_item(ix))
                .unwrap_or_else(|| {
                    panic!("op-log row {ix} was not laid out — is the Operation Log tab open?")
                })
        };
        let collapsed0 = row_bounds(cx, 0);
        let collapsed1 = row_bounds(cx, 1);
        assert!(
            collapsed1.origin.y >= collapsed0.origin.y + collapsed0.size.height,
            "collapsed rows already overlap: {collapsed0:?} / {collapsed1:?}"
        );

        // Expand row 0 (what a click on the row does).
        panel.update(cx, |p, cx| {
            p.toggle_expanded(0);
            cx.notify();
        });
        cx.run_until_parked();

        let expanded0 = row_bounds(cx, 0);
        let expanded1 = row_bounds(cx, 1);
        assert!(
            expanded0.size.height > collapsed0.size.height,
            "expanding row 0 must grow the row itself (was {:?}, now {:?}) — a \
             fixed-height list would keep it at the summary height",
            collapsed0.size.height,
            expanded0.size.height
        );
        assert!(
            expanded1.origin.y > collapsed1.origin.y,
            "expanding row 0 must push row 1 down (row 1 y {:?} -> {:?})",
            collapsed1.origin.y,
            expanded1.origin.y
        );
        assert!(
            expanded1.origin.y >= expanded0.origin.y + expanded0.size.height,
            "expanded row 0 overlaps row 1: {expanded0:?} / {expanded1:?}"
        );

        // The row copy button's handler: the whole entry, not the truncated line.
        let expected = cx.read(|app| {
            let p = panel.read(app);
            oplog_panel::entry_clipboard_text(&p.entries()[0])
        });
        panel.update(cx, |p, cx| p.copy_entry(0, cx));
        let copied = cx
            .read_from_clipboard()
            .and_then(|i| i.text())
            .expect("clipboard text");
        assert_eq!(
            copied, expected,
            "copy_entry should write the formatted entry"
        );
        // Independent of the formatter itself: the clipboard must carry the
        // header AND every detail line, 200-char error included — the tail the
        // truncated summary row never shows.
        cx.read(|cx| assert_eq!(panel.read(cx).entries()[0].op, "checkout"));
        assert!(
            copied.contains(&long_error),
            "the copied entry must carry the full 200-char error detail line"
        );
        assert!(
            copied.lines().count() >= 4,
            "expected header + before/dirty/error lines, got {}: {copied:?}",
            copied.lines().count()
        );

        assert_eq!(
            before_fp,
            repo_fingerprint(&repo_path),
            "repo mutated during a read-only scenario"
        );
        drop(panel);
        unmount(cx, kagi, win);
        eprintln!(
            "[gui-e2e] PASS oplog_expand_copy grew {:?} -> {:?}, row1 {:?} -> {:?}, copied {} chars",
            collapsed0.size.height,
            expanded0.size.height,
            collapsed1.origin.y,
            expanded1.origin.y,
            copied.chars().count()
        );
    }

    /// Issue #335: the `CreateSnapshot` command captures the working tree as a
    /// non-destructive savepoint under `refs/kagi/snapshots/`. Asserts no such
    /// ref exists before, exactly one after, and that HEAD/porcelain are
    /// unchanged (a snapshot only adds a ref — it never moves HEAD).
    fn scenario_create_snapshot(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let before_fp = repo_fingerprint(&repo_path);
        let (kagi, win) = mount(cx, &repo_path);

        assert!(
            for_each_ref(&repo_path, "refs/kagi/snapshots/")
                .trim()
                .is_empty(),
            "no snapshot ref should exist before CreateSnapshot"
        );

        cx.dispatch_action(win, CreateSnapshot);
        cx.run_until_parked();

        let refs = for_each_ref(&repo_path, "refs/kagi/snapshots/");
        let count = refs.lines().filter(|l| !l.trim().is_empty()).count();
        assert_eq!(
            count, 1,
            "CreateSnapshot should add exactly one refs/kagi/snapshots/ ref, got:\n{refs}"
        );
        assert_eq!(
            before_fp,
            repo_fingerprint(&repo_path),
            "CreateSnapshot must not move HEAD or dirty the working tree"
        );
        unmount(cx, kagi, win);
        eprintln!("[gui-e2e] PASS create_snapshot ref_count={count}");
    }

    /// Issue #373: the command palette's SetTheme action switches the active
    /// theme (`KagiApp::set_theme`, the exact method the palette dispatches).
    /// Asserts the process-wide active-theme slug flips to the requested theme.
    fn scenario_theme_switch(cx: &mut VisualTestAppContext) {
        let fixture = build_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let (kagi, win) = mount(cx, &repo_path);

        let before = theme::theme().slug.to_string();
        let saved = kagi::ui::settings::read_setting("theme");
        let target = if before == "dracula" {
            "tokyo-night"
        } else {
            "dracula"
        };
        kagi.update(cx, |app, cx| app.set_theme(target, cx));
        cx.run_until_parked();

        let after = theme::theme().slug.to_string();
        assert_eq!(
            after, target,
            "SetTheme should make {target} the active theme"
        );
        assert_ne!(after, before, "active theme should have changed");
        // Put the theme back for the scenarios that follow (#516): the
        // active theme, gpui_component's copy, and the saved key.
        kagi.update(cx, |app, cx| app.set_theme(&before, cx));
        kagi::ui::settings::write_setting("theme", saved.as_deref());
        unmount(cx, kagi, win);
        eprintln!("[gui-e2e] PASS theme_switch {before} -> {after}");
    }

    /// Issue #337: `CommitRow.provenance` is computed through the real snapshot
    /// pipeline. Asserts the agent commit's row classifies as Claude Code and
    /// the plain human commit's row carries no provenance.
    fn scenario_agent_provenance(cx: &mut VisualTestAppContext) {
        let fixture = build_agent_fixture();
        let repo_path = fixture.path().canonicalize().unwrap();
        let (kagi, win) = mount(cx, &repo_path);

        let (head_agent, parent_agent) = cx.read(|app| {
            let rows = &kagi.read(app).view().rows;
            (
                rows[0]
                    .provenance
                    .as_ref()
                    .map(|p| p.agent.label().to_string()),
                rows[1]
                    .provenance
                    .as_ref()
                    .map(|p| p.agent.label().to_string()),
            )
        });
        assert_eq!(
            head_agent.as_deref(),
            Some("Claude Code"),
            "agent commit (HEAD) should classify as Claude Code"
        );
        assert_eq!(
            parent_agent, None,
            "plain human commit should carry no provenance"
        );
        unmount(cx, kagi, win);
        eprintln!("[gui-e2e] PASS agent_provenance head=ClaudeCode human=None");
    }

    /// Issue #472, with #767's anchor colours: each WIP row draws a dashed
    /// connector down to its own worktree's HEAD, in the lane colour of the
    /// HEAD it lands on.
    ///
    /// Asserts, through the real snapshot → `build_tab_view` pipeline: two WIP
    /// rows each get an anchor, the two lanes differ, every row above the open
    /// repo's HEAD carries exactly one WIP-ghost `Pass` on that lane, the HEAD
    /// row carries the `IntoNode` landing on HEAD's own node, and each
    /// connector's colour index is its HEAD row's `node_color`. That colour
    /// used to be the worktree's index in `worktrees` — a number that said
    /// nothing about where the line ends; #767 makes the anchor take the
    /// colour of the lane it points at, which is what pairs the hollow node
    /// with its swimlane on screen.
    fn scenario_wip_head_connector(cx: &mut VisualTestAppContext) {
        let (_fixture, repo_path, _wt_path) = build_wip_connector_fixture();
        let before_fp = repo_fingerprint(&repo_path);
        let (kagi, win) = mount(cx, &repo_path);

        // Same lane/colour may serve multiple HEADs; the trace continues to
        // the deepest one instead of allocating another column.
        fn ghost(
            row: &commit_list::CommitRow,
            kind: Option<EdgeKind>,
            from_lane: usize,
            color: usize,
        ) -> Vec<&GraphEdge> {
            row.edges
                .iter()
                .filter(|e| {
                    kind.as_ref().is_none_or(|kind| e.kind == *kind)
                        && e.from_lane == from_lane
                        && graph_wip::wip_color_index(e.color) == Some(color)
                })
                .collect()
        }

        fn ghost_color(edge: &GraphEdge) -> usize {
            graph_wip::wip_color_index(edge.color).expect("filtered to WIP-ghost edges")
        }

        cx.read(|app| {
            let view = &kagi.read(app).view();

            // Two dirty working trees → two WIP rows, each with an anchor.
            assert_eq!(
                view.wip_lanes.len(),
                2,
                "expected 2 WIP rows (open repo + linked worktree), got {:?}",
                view.wip_lanes
            );
            // #476 slice 2: the map is keyed by target, not by row position.
            let anchor_for = |target: graph_wip::WipTarget| {
                view.wip_lanes
                    .iter()
                    .find(|(t, _)| *t == target)
                    .and_then(|(_, a)| *a)
            };
            let open = anchor_for(graph_wip::WipTarget::Current)
                .expect("open repo's WIP row got no anchor");
            let wt_anchor = anchor_for(graph_wip::WipTarget::Worktree(1))
                .expect("linked worktree's WIP row got no anchor");

            // The open repo's HEAD is buried under the `ahead` branch.
            let head = view
                .rows
                .iter()
                .position(|r| r.is_head)
                .expect("no HEAD row");
            assert!(
                head >= 3,
                "fixture should bury HEAD at row >= 3, got row {head}"
            );
            let head_color = view.rows[head].node_color;
            assert_eq!(open.lane, view.rows[head].lane);
            assert_eq!(
                open.color, head_color,
                "#767: the anchor's colour is the colour of the HEAD lane it points at"
            );

            // Every row above HEAD carries the connector down its own lane.
            for (i, row) in view.rows[..head].iter().enumerate() {
                let passes = ghost(row, Some(EdgeKind::Pass), open.lane, open.color);
                assert_eq!(
                    passes.len(),
                    1,
                    "row {i} above HEAD must carry exactly one WIP-ghost Pass"
                );
                assert_eq!(passes[0].to_lane, open.lane);
                assert_eq!(
                    ghost_color(passes[0]),
                    head_color,
                    "row {i}'s connector must be drawn in HEAD's lane colour"
                );
            }
            let into = ghost(&view.rows[head], None, open.lane, open.color);
            assert_eq!(into.len(), 1, "HEAD's row must carry the shared connector");
            assert_eq!(
                into[0].to_lane, view.rows[head].lane,
                "the connector must reach HEAD's own node"
            );
            assert_eq!(ghost_color(into[0]), head_color);

            // The linked worktree's connector reaches ITS head, in ITS head's
            // lane colour.
            let wt = &view.worktrees[1];
            assert!(!wt.is_current, "worktrees[1] should be the linked worktree");
            let wt_head_id = wt.head.clone().expect("linked worktree HEAD not read");
            let wt_head = view.commit_row_index[&wt_head_id];
            assert_ne!(
                wt_head, head,
                "the two worktrees should sit on different commits"
            );
            let wt_head_color = view.rows[wt_head].node_color;
            assert_eq!(wt_anchor.lane, view.rows[wt_head].lane);
            assert_eq!(
                wt_anchor.color, wt_head_color,
                "#767: the worktree's anchor takes its own HEAD row's lane colour"
            );
            let wt_into = ghost(&view.rows[wt_head], None, wt_anchor.lane, wt_anchor.color);
            assert_eq!(
                wt_into.len(),
                1,
                "the linked worktree's HEAD must lie on its connector"
            );
            assert_eq!(
                wt_into[0].to_lane, view.rows[wt_head].lane,
                "the connector must reach that worktree's HEAD node"
            );
            assert_eq!(ghost_color(wt_into[0]), wt_head_color);

            eprintln!(
                "[gui-e2e] PASS wip_head_connector head_row={head} lanes={}/{} colours={}/{} \
                 wt_head_row={wt_head}",
                open.lane, wt_anchor.lane, open.color, wt_anchor.color
            );
        });

        assert_eq!(
            before_fp,
            repo_fingerprint(&repo_path),
            "repo mutated during a read-only scenario"
        );
        unmount(cx, kagi, win);
    }

    /// Issue #473 + #476 slice 1: clicking a LINKED WORKTREE's WIP row shows
    /// that worktree's changes in the commit panel **in place** — no new tab,
    /// no snapshot — and the four staging ops dispatched there write into
    /// **that worktree**, not the open tab's repository.
    ///
    /// Drives the row's own click handler (`open_commit_panel_for_worktree`)
    /// and asserts: `tabs.len()` unchanged; the panel points at the WORKTREE
    /// and lists ITS dirty file; `do_stage_all` / `do_unstage_all` /
    /// `do_stage_file` / `do_unstage_file` each move the WORKTREE's
    /// `git status --porcelain` while the OPEN repo's fingerprint is unchanged;
    /// the worktree's WIP row counts (`view().worktrees[i].wip`) follow;
    /// the still-tab-resolved ops (commit plan, commit, amend, discard-all) are
    /// still refused and leave both trees untouched; and the watcher's in-place
    /// refresh (`refresh_working_tree_external`) does not swap the panel back to
    /// the open repository.
    fn scenario_worktree_wip_inline(cx: &mut VisualTestAppContext) {
        let (_fixture, repo_path, wt_path) = build_wip_connector_fixture();
        let repo_fp = repo_fingerprint(&repo_path);
        let wt_fp = repo_fingerprint(&wt_path);
        let (kagi, win) = mount(cx, &repo_path);

        let tabs_before = cx.read(|app| kagi.read(app).tabs.len());

        // What the linked worktree's WIP row click does (`render_wip.rs`),
        // minus the message `InputState`s — see `open_worktree_panel_no_inputs`
        // (#476 slice 2 gives a worktree panel real inputs, and the runner
        // cannot construct one without failing the run's leak detector). Both
        // paths go through the same `attach_commit_panel_at`, which is where
        // the panel's `repo_path` canonicalization and `foreign` marking live.
        let wt = wt_path.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, wt, "ahead", 1, cx)
        });
        cx.run_until_parked();

        cx.read(|app| {
            let app = kagi.read(app);
            assert_eq!(
                app.tabs.len(),
                tabs_before,
                "clicking a linked worktree's WIP row must NOT open a tab"
            );
            assert!(
                app.ui().commit_panel_open,
                "the commit panel should be open"
            );
        });
        let (panel_repo, files, foreign) = cx.read(|app| {
            let p = kagi.read(app).ui().commit_panel.clone().expect("panel");
            let p = p.read(app);
            (
                p.repo_path.clone(),
                p.state
                    .unstaged
                    .iter()
                    .map(|f| f.path.display().to_string())
                    .collect::<Vec<_>>(),
                p.foreign.clone(),
            )
        });
        assert_eq!(
            panel_repo, wt_path,
            "the panel must point at the worktree, not the open repo"
        );
        assert!(
            files.iter().any(|f| f == "dirty.txt"),
            "the panel should list the WORKTREE's dirty file, got {files:?}"
        );
        assert!(
            foreign.is_some(),
            "a worktree panel must be marked foreign (read-only + header chip)"
        );
        assert!(
            cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "commit_panel_is_foreign must hold for a worktree panel"
        );

        // ── #476 slice 1: the staging ops write into the WORKTREE ──────────
        // The worktree's WIP row counts, straight off `view()` — they come
        // from the snapshot, so this is what proves the row followed the write.
        let wip_of = |cx: &mut VisualTestAppContext| {
            cx.read(|app| {
                kagi.read(app)
                    .view()
                    .worktrees
                    .iter()
                    .find(|w| w.path == wt_path)
                    .unwrap_or_else(|| panic!("no worktree row for {}", wt_path.display()))
                    .wip
                    .expect("the dirty worktree must carry a WIP row")
            })
        };
        let wip_before = wip_of(cx);
        assert_eq!(
            (wip_before.staged, wip_before.untracked),
            (0, 1),
            "fixture: the worktree starts with one untracked file"
        );

        kagi.update(cx, |app, cx| {
            app.do_stage_all(app.active_session().unwrap(), cx)
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by a stage dispatched at a worktree panel"
        );
        assert_eq!(
            repo_fingerprint(&wt_path).1,
            "A  dirty.txt\n",
            "stage-all from a worktree panel must stage in the WORKTREE"
        );
        let wip_staged = wip_of(cx);
        assert_eq!(
            (wip_staged.staged, wip_staged.unstaged, wip_staged.untracked),
            (1, 0, 0),
            "the worktree's WIP row counts must follow the stage"
        );

        kagi.update(cx, |app, cx| {
            app.do_unstage_all(app.active_session().unwrap(), cx)
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_path),
            wt_fp,
            "unstage-all from a worktree panel must unstage in the WORKTREE"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by an unstage dispatched at a worktree panel"
        );
        assert_eq!(
            wip_of(cx).untracked,
            1,
            "the WIP row must follow back to untracked"
        );

        // Per-file, by the same panel index the row's button carries.
        kagi.update(cx, |app, cx| {
            app.do_stage_file(app.active_session().unwrap(), 0, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_path).1,
            "A  dirty.txt\n",
            "stage(0) from a worktree panel must stage in the WORKTREE"
        );
        kagi.update(cx, |app, cx| {
            app.do_unstage_file(app.active_session().unwrap(), 0, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_path),
            wt_fp,
            "unstage(0) from a worktree panel must unstage in the WORKTREE"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by a per-file stage/unstage at a worktree panel"
        );

        // ── Planning amend / discard-all writes nothing ──────────────────────
        // #476 slice 3 converted both (execution lives in
        // `scenario_worktree_panel_amend_discard`). Opening their confirms is
        // still a pure read — `plan → confirm → …` means nothing has happened
        // until the second click, in either repository.
        kagi.update(cx, |app, cx| {
            let owner = app.active_session().unwrap();
            app.commit_panel_amend(owner, cx);
            app.open_discard_all_modal(owner, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by merely PLANNING a write at a worktree panel"
        );
        assert_eq!(
            wt_fp,
            repo_fingerprint(&wt_path),
            "the WORKTREE was mutated by merely PLANNING a write at a worktree panel"
        );

        // The watcher's in-place refresh must not swap the panel back.
        kagi.update(cx, |app, cx| app.refresh_working_tree_external(cx));
        cx.run_until_parked();
        let after_reload = cx.read(|app| {
            let p = kagi
                .read(app)
                .ui()
                .commit_panel
                .clone()
                .expect("panel survived");
            p.read(app).repo_path.clone()
        });
        assert_eq!(
            after_reload, wt_path,
            "a watcher refresh replaced the worktree panel with the open repo's"
        );
        assert_eq!(
            cx.read(|app| kagi.read(app).tabs.len()),
            tabs_before,
            "no tab may appear at any point in this scenario"
        );

        // POSITIVE CONTROL: the guard must not misfire on the tab's OWN panel.
        // Everything above proves writes are refused; this proves they are
        // refused *because the panel is foreign*, not because staging is broken
        // in the harness — and pins `is_foreign_panel`'s path-equality down.
        //
        // The panel is re-pointed at the tab's repo rather than opened through
        // `open_commit_panel`: that path builds the two `InputState`s, and
        // gpui_component's `InputState` retains handles that gpui's end-of-run
        // leak detector then reports (nothing in this suite drops them). What
        // is under test here is the guard + the staging path, and both run
        // identically either way.
        let panel = cx.read(|app| kagi.read(app).ui().commit_panel.clone().expect("panel"));
        let own = repo_path.clone();
        panel.update(cx, |v, _| {
            v.repo_path = own.clone();
            v.foreign = None;
            v.reload_status(&own);
        });
        cx.run_until_parked();
        assert!(
            !cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "the open repo's own panel must NOT be treated as foreign"
        );
        kagi.update(cx, |app, cx| {
            app.do_stage_all(app.active_session().unwrap(), cx)
        });
        cx.run_until_parked();
        let staged_fp = repo_fingerprint(&repo_path);
        assert_ne!(
            repo_fp, staged_fp,
            "staging from the OPEN repo's own panel must actually stage"
        );
        assert_eq!(
            staged_fp.1, "A  dirty.txt\n",
            "expected dirty.txt staged in the open repo, got {:?}",
            staged_fp.1
        );
        assert_eq!(
            wt_fp,
            repo_fingerprint(&wt_path),
            "staging in the open repo must not touch the worktree"
        );

        drop(panel);
        unmount(cx, kagi, win);
        eprintln!(
            "[gui-e2e] PASS worktree_wip_inline tabs={tabs_before} panel={} files={files:?} \
             own-panel-staged={:?}",
            panel_repo.display(),
            staged_fp.1.trim()
        );
    }

    /// Issue #476 slice 2: a commit dispatched from a LINKED WORKTREE's panel
    /// lands in **that worktree**.
    ///
    /// Drives the real write path — `do_stage_all`, then
    /// `open_commit_plan_modal`, which plans and (no blockers) runs
    /// `start_commit` itself, the "smooth commit" the Commit button takes — and
    /// asserts: the worktree's HEAD advanced by exactly one commit and its
    /// working tree is clean; the OPEN repository's HEAD/porcelain and the
    /// second worktree are untouched; the newest oplog entry (in-memory panel
    /// AND the persisted log `Backend::run` writes) is `commit` against the
    /// WORKTREE's path; the open tab's graph gained the new commit (shared refs
    /// and the post-commit re-snapshot); the committed worktree's WIP row is gone
    /// while the surviving rows keep their own lanes (the `WipTarget` keying);
    /// and the tab's Undo does not point at the worktree's new commit.
    fn scenario_worktree_panel_commit(cx: &mut VisualTestAppContext) {
        let (_fixture, repo_path, wt_a, wt_b) = build_two_worktree_fixture();
        let repo_fp = repo_fingerprint(&repo_path);
        let wt_b_fp = repo_fingerprint(&wt_b);
        let wt_a_head_before = repo_fingerprint(&wt_a).0;
        let wt_a_commits_before = commit_count(&wt_a);
        let (kagi, win) = mount(cx, &repo_path);

        // Where each worktree sits in the snapshot's list — its `WipTarget` key
        // and its lane colour index are both that position.
        let wt_index = |cx: &mut VisualTestAppContext, path: &Path| -> usize {
            cx.read(|app| {
                kagi.read(app)
                    .view()
                    .worktrees
                    .iter()
                    .position(|w| w.path == path)
                    .unwrap_or_else(|| panic!("no worktree row for {}", path.display()))
            })
        };
        let idx_a = wt_index(cx, &wt_a);
        let idx_b = wt_index(cx, &wt_b);
        let (lanes_before, lane_open, lane_a, lane_b) = cx.read(|app| {
            let lanes = kagi.read(app).view().wip_lanes.clone();
            let get = |t| graph_wip::wip_lane(&lanes, t);
            (
                lanes.clone(),
                get(graph_wip::WipTarget::Current),
                get(graph_wip::WipTarget::Worktree(idx_a)),
                get(graph_wip::WipTarget::Worktree(idx_b)),
            )
        });
        assert_eq!(
            lanes_before.len(),
            3,
            "fixture: three dirty working trees → three WIP rows, got {lanes_before:?}"
        );
        assert!(
            lane_open.is_some() && lane_a.is_some() && lane_b.is_some(),
            "every WIP row should have drawn a connector: {lanes_before:?}"
        );

        // Open worktree A's panel, stage its file, give it a message.
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "the panel must be marked foreign for the write ops to resolve it"
        );
        kagi.update(cx, |app, cx| {
            app.do_stage_all(app.active_session().unwrap(), cx);
            e2e::set_commit_message(app, "wt-a: from the worktree panel", cx);
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_a).1,
            "A  dirty.txt\n",
            "stage-all should have staged in the WORKTREE"
        );

        // The Commit button's path: plan, then (no blockers) commit.
        kagi.update(cx, |app, cx| {
            app.open_commit_plan_modal(app.active_session().unwrap(), cx)
        });
        cx.run_until_parked();

        // ── The worktree moved; nothing else did ────────────────────────────
        let (wt_a_head_after, wt_a_status) = repo_fingerprint(&wt_a);
        assert_ne!(
            wt_a_head_after, wt_a_head_before,
            "the WORKTREE's HEAD must have advanced"
        );
        assert_eq!(
            commit_count(&wt_a),
            wt_a_commits_before + 1,
            "exactly one new commit in the worktree"
        );
        assert_eq!(
            wt_a_status, "",
            "the worktree should be clean after committing its only change"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by a commit dispatched at a worktree panel"
        );
        assert_eq!(
            wt_b_fp,
            repo_fingerprint(&wt_b),
            "the OTHER worktree was mutated by a commit at worktree A's panel"
        );

        // ── The oplog is attributed to the worktree ─────────────────────────
        let (op, repo) = cx.read(|app| {
            let panel = kagi.read(app).op_log.clone().expect("op_log entity");
            let panel = panel.read(app);
            e2e::entry_op_and_repo(panel.entries().front().expect("an op-log entry"))
        });
        assert_eq!(op, "commit", "newest op-log entry should be the commit");
        assert_eq!(
            PathBuf::from(&repo),
            wt_a,
            "the op-log entry's repo must be the WORKTREE's path, not the tab's"
        );
        let (pop, prepo) = e2e::latest_persisted_op().expect("a persisted oplog entry");
        assert_eq!(pop, "commit");
        assert_eq!(
            PathBuf::from(&prepo),
            wt_a,
            "the persisted oplog entry's repo must be the WORKTREE's path"
        );

        // ── The open tab's graph gained the commit (shared refs) ────────────
        let new_sha = wt_a_head_after.clone();
        assert!(
            cx.read(|app| kagi.read(app).view().rows.iter().any(|r| r.id.0 == new_sha)),
            "the open tab must re-snapshot and show the worktree's new commit"
        );

        // ── The WIP rows: A's is gone, the others keep their own lanes ──────
        let wip_a = cx.read(|app| {
            kagi.read(app)
                .view()
                .worktrees
                .iter()
                .find(|w| w.path == wt_a)
                .and_then(|w| w.wip)
        });
        assert!(
            wip_a.is_none_or(|w| !w.is_dirty()),
            "worktree A is clean after the commit — its WIP row must be gone, got {wip_a:?}"
        );
        // The frame between the commit and the re-snapshot renders the NEW row
        // list against the OLD lane map — `refresh_worktree_wip_row` drops A's
        // row in place. Keyed by target, the survivors keep their own lanes;
        // by position, the open repo's row would hand its lane to A's former
        // neighbour and worktree B would inherit A's.
        let rows_now = [
            graph_wip::WipTarget::Current,
            graph_wip::WipTarget::Worktree(idx_b),
        ];
        assert_eq!(
            graph_wip::lanes_for_rows(&lanes_before, &rows_now)
                .into_iter()
                .map(|anchor| anchor.map(|a| a.lane))
                .collect::<Vec<_>>(),
            vec![lane_open, lane_b],
            "a committed worktree's row leaving the list must not shift the \
             lanes of the rows that remain (lanes were {lanes_before:?})"
        );
        // …and the freshly-built map agrees, with A's entry simply absent.
        cx.read(|app| {
            let lanes = &kagi.read(app).view().wip_lanes;
            assert_eq!(
                graph_wip::wip_lane(lanes, graph_wip::WipTarget::Worktree(idx_a)),
                None,
                "the clean worktree must have no connector: {lanes:?}"
            );
            assert_eq!(
                graph_wip::wip_lane(lanes, graph_wip::WipTarget::Current),
                lane_open,
                "the open repo's WIP row must keep its lane: {lanes:?}"
            );
        });

        // ── The commit recorded NO undo entry for the tab ───────────────────
        // #476 slice 3: `operation_history` is per tab, and `head_branch_and_sha`
        // reads the TAB's HEAD — so a recorded worktree commit would sit here as
        // the tab's branch pointed at the worktree's commit, and Cmd+Z would
        // move `main` onto it. (The stack is not compared wholesale: `reload`
        // legitimately seeds it from the tab's own reflog, ADR-0084.)
        let undo_head = cx.read(|app| e2e::undo_head(kagi.read(app)));
        assert!(
            undo_head
                .as_ref()
                .is_none_or(|(_, after, _)| after != &wt_a_head_after),
            "the tab's Undo must not point at the WORKTREE's commit: {undo_head:?}"
        );

        unmount(cx, kagi, win);
        eprintln!(
            "[gui-e2e] PASS worktree_panel_commit wt-a {}..{} lanes open={lane_open:?} \
             b={lane_b:?} oplog={op}@{repo}",
            &wt_a_head_before[..8],
            &wt_a_head_after[..8],
        );
    }

    /// Issue #476 slice 3: **amend** and **discard** dispatched from a LINKED
    /// WORKTREE's panel act on that worktree.
    ///
    /// Both use one explicit confirmation for their recovery-backed local plans:
    /// `commit_panel_amend` → `start_amend`, and `open_discard_all_modal`
    /// → `start_discard`.
    ///
    /// Asserts, in order: the amend rewrote worktree A's HEAD **in place** (new
    /// SHA, same parent, same commit count, clean tree) while the open
    /// repository and worktree B are untouched and the persisted oplog names A;
    /// the open tab re-snapshotted onto the rewritten commit; the discard
    /// emptied A's working tree with a backup blob that resolves in the shared
    /// ODB; and neither op pushed anything onto the TAB's undo stack.
    fn scenario_worktree_panel_amend_discard(cx: &mut VisualTestAppContext) {
        let (_fixture, repo_path, wt_a, wt_b) = build_two_worktree_fixture();
        let repo_fp = repo_fingerprint(&repo_path);
        let wt_b_fp = repo_fingerprint(&wt_b);
        let head_before = repo_fingerprint(&wt_a).0;
        let parent_before = rev_parse(&wt_a, "HEAD^");
        let commits_before = commit_count(&wt_a);
        let (kagi, win) = mount(cx, &repo_path);

        let idx_a = cx.read(|app| {
            kagi.read(app)
                .view()
                .worktrees
                .iter()
                .position(|w| w.path == wt_a)
                .unwrap_or_else(|| panic!("no worktree row for {}", wt_a.display()))
        });
        // ── (a) amend ───────────────────────────────────────────────────────
        // Stage A's file, then amend: staged content, no new message
        // (`AmendMode::Staged`), so A's HEAD is replaced rather than extended.
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "the panel must be marked foreign for the write ops to resolve it"
        );
        kagi.update(cx, |app, cx| {
            app.do_stage_all(app.active_session().unwrap(), cx)
        });
        cx.run_until_parked();

        kagi.update(cx, |app, cx| {
            app.commit_panel_amend(app.active_session().unwrap(), cx)
        });
        cx.run_until_parked();
        kagi::ui::dialog_a11y::clear_recorded_a11y();
        kagi.update(cx, |_, cx| cx.notify());
        cx.update_window(win, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        let title = cx.read(|cx| {
            kagi_ui_core::i18n::plan_title_text(
                &kagi.read(cx).amend_modal().expect("amend card").plan.title,
            )
        });
        let expected = kagi::ui::worktree_wip::worktree_modal_title(&title, Some("wt-a"));
        let dialog = kagi::ui::dialog_a11y::recorded_dialog("amend-card")
            .expect("linked worktree amend card drawn");
        assert_eq!(
            dialog.label, expected,
            "AX title must name the target worktree"
        );
        assert_eq!(
            head_before,
            repo_fingerprint(&wt_a).0,
            "planning the amend must not have rewritten anything yet"
        );
        assert!(!cx.read(|cx| kagi.read(cx).amend_modal().unwrap().requires_two_stage()));
        kagi.update(cx, |app, cx| app.start_amend(cx));
        cx.run_until_parked();

        let (head_after_amend, status_after_amend) = repo_fingerprint(&wt_a);
        assert_ne!(
            head_after_amend, head_before,
            "the WORKTREE's HEAD must have been rewritten"
        );
        assert_eq!(
            rev_parse(&wt_a, "HEAD^"),
            parent_before,
            "an amend replaces the tip in place — the parent must not move"
        );
        assert_eq!(
            commit_count(&wt_a),
            commits_before,
            "an amend must not add a commit"
        );
        assert_eq!(
            status_after_amend, "",
            "the staged change went INTO the amended commit, so A is clean"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by an amend dispatched at a worktree panel"
        );
        assert_eq!(
            wt_b_fp,
            repo_fingerprint(&wt_b),
            "the OTHER worktree was mutated by an amend at worktree A's panel"
        );
        let (pop, prepo) = e2e::latest_persisted_op().expect("a persisted oplog entry");
        assert_eq!(
            pop, "amend",
            "newest persisted oplog entry should be the amend"
        );
        assert_eq!(
            PathBuf::from(&prepo),
            wt_a,
            "the persisted oplog entry's repo must be the WORKTREE's path, not the tab's"
        );
        // Shared refs + the post-amend re-snapshot: the open tab shows the new tip.
        assert!(
            cx.read(|app| kagi
                .read(app)
                .view()
                .rows
                .iter()
                .any(|r| r.id.0 == head_after_amend)),
            "the open tab must re-snapshot and show the worktree's amended commit"
        );

        // ── (b) discard ─────────────────────────────────────────────────────
        // `reload` dropped the (now clean) panel, so dirty A again and re-open it.
        std::fs::write(wt_a.join("f.txt"), "dirtied for discard\n").unwrap();
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_a).1,
            " M f.txt\n",
            "fixture: worktree A must have exactly one unstaged modification to discard"
        );

        kagi.update(cx, |app, cx| {
            app.open_discard_all_modal(app.active_session().unwrap(), cx)
        });
        cx.run_until_parked();
        assert!(!cx.read(|cx| kagi.read(cx).discard_modal().unwrap().requires_two_stage()));
        kagi.update(cx, |app, cx| app.start_discard(cx));
        cx.run_until_parked();

        assert_eq!(
            repo_fingerprint(&wt_a),
            (head_after_amend.clone(), String::new()),
            "discard-all from a worktree panel must clean the WORKTREE without moving its HEAD"
        );
        assert_eq!(
            repo_fp,
            repo_fingerprint(&repo_path),
            "the OPEN repo was mutated by a discard dispatched at a worktree panel"
        );
        assert_eq!(
            wt_b_fp,
            repo_fingerprint(&wt_b),
            "the OTHER worktree was mutated by a discard at worktree A's panel"
        );
        let (pop, prepo) = e2e::latest_persisted_op().expect("a persisted oplog entry");
        assert_eq!(
            pop, "discard",
            "newest persisted oplog entry should be the discard"
        );
        assert_eq!(
            PathBuf::from(&prepo),
            wt_a,
            "the persisted oplog entry's repo must be the WORKTREE's path, not the tab's"
        );
        // The recovery handle (ADR-0083): the pre-discard content lives in the
        // ODB as a blob, and the oplog entry carries its SHA. The worktree
        // shares the open repository's ODB, so the blob resolves from BOTH.
        //
        // The in-memory entry is presentation only — it says which repo the op
        // named. The recovery material is read from the PERSISTED receipt's
        // typed `recovery` handles (#500), never by splitting the English
        // `backup: <path>=<sha>` summary out of `after.dirty`.
        let (op, repo) = cx.read(|app| {
            let panel = kagi.read(app).op_log.clone().expect("op_log entity");
            let panel = panel.read(app);
            let entry = panel.entries().front().expect("an op-log entry");
            e2e::entry_op_and_repo(entry)
        });
        assert_eq!(op, "discard", "newest op-log entry should be the discard");
        assert_eq!(
            PathBuf::from(&repo),
            wt_a,
            "the op-log entry's repo must be the WORKTREE's path"
        );
        let receipt = kagi_git::oplog::read_oplog_tail_for_repo(&wt_a, 1)
            .pop()
            .expect("persisted discard receipt");
        assert_eq!(receipt.op, "discard");
        assert_eq!(receipt.backup_refs.len(), 1);
        let handle = receipt
            .recovery
            .iter()
            .find(|h| h.path.as_deref() == Some("f.txt"))
            .unwrap_or_else(|| panic!("no typed f.txt backup handle: {:?}", receipt.recovery));
        assert_eq!(handle.kind, kagi_git::oplog::recovery::FILE_BACKUP);
        let blob = handle.oid.as_str();
        let reference = handle
            .reference
            .as_deref()
            .expect("a file backup handle names its GC root");
        assert_eq!(
            Some(reference),
            receipt.backup_refs.first().map(String::as_str),
            "the typed handle's ref must be the receipt's retained root"
        );
        assert!(reference.starts_with("refs/kagi/backups/"));
        assert_eq!(blob.len(), 40, "expected a 40-hex blob SHA, got {blob:?}");
        for odb in [&wt_a, &repo_path] {
            assert_eq!(
                rev_parse(odb, &format!("{reference}:file")),
                blob,
                "receipt tree entry must resolve to the logged backup blob in {}",
                odb.display()
            );
            let tree = Command::new("git")
                .current_dir(odb)
                .args(["ls-tree", reference])
                .output()
                .unwrap();
            assert!(tree.status.success());
            assert_eq!(
                tree.stdout,
                format!("100644 blob {blob}\tfile\n").as_bytes()
            );
            assert!(
                git_ok(odb, &["cat-file", "-e", blob]),
                "the discard's backup blob {blob} must resolve in {}",
                odb.display()
            );
        }

        // ── (c) neither op touched the TAB's undo stack ─────────────────────
        // Both ran in another repository, so `undo_skipped_for_foreign` kept
        // them off `operation_history` (and said so on stderr:
        // `[kagi] undo: skipped — amend ran in another worktree …`).
        //
        // `reload` seeds an empty stack from the TAB's own branch reflog
        // (ADR-0084 §2), and a seeded entry is undoable by contract: the top
        // of the stack must be `main`'s own last commit, and running it is a
        // soft move of `main` alone (§3) — never `main` onto worktree A's
        // amended commit, never a write in either worktree. (Until #773 the
        // fixture's `main` had only its initial commit, whose zero `before`
        // made the Undo a refused no-op, and this section asserted that
        // nothing moved at all; #849.)
        let main_head = repo_fp.0.clone();
        let main_parent = rev_parse(&repo_path, "HEAD^");
        let undo_head = cx.read(|app| e2e::undo_head(kagi.read(app)));
        assert!(
            undo_head
                .as_ref()
                .is_some_and(|(branch, after, _)| branch == "main" && after == &main_head),
            "the tab's Undo must hold main's own reflog entry (main at {main_head}), \
             not the WORKTREE's amend ({head_after_amend}): {undo_head:?}"
        );
        kagi.update(cx, |app, cx| {
            app.open_history_undo_modal();
            app.confirm_history(cx);
        });
        cx.run_until_parked();
        assert_eq!(
            (
                repo_fingerprint(&repo_path),
                repo_fingerprint(&wt_a),
                repo_fingerprint(&wt_b)
            ),
            (
                (main_parent, repo_fp.1.clone()),
                (head_after_amend.clone(), String::new()),
                wt_b_fp.clone()
            ),
            "the tab's Undo is a soft move of main alone; neither worktree is touched"
        );
        // Redo puts main back, so (d) starts from the fixture state again.
        kagi.update(cx, |app, cx| {
            app.open_history_redo_modal();
            app.confirm_history(cx);
        });
        cx.run_until_parked();
        assert_eq!(
            (repo_fingerprint(&repo_path), repo_fingerprint(&wt_a)),
            (repo_fp.clone(), (head_after_amend.clone(), String::new())),
            "the tab's Redo returns main to its own tip and leaves the worktree alone"
        );

        // ── (d) the Editor Workspace tree discards in the TAB ───────────────
        // Slice 3 review: `open_discard_modal_for_path` is shared by the commit
        // panel's file menu and the editor tree, which mean DIFFERENT
        // repositories. With the same relative path dirty in both, resolving
        // the panel would destroy the worktree's copy on an editor-tree click.
        // `WriteOrigin` is what keeps them apart, and it rides on the modal so
        // preflight + execute land where the plan was built.
        std::fs::write(repo_path.join("f.txt"), "open repo edit\n").unwrap();
        std::fs::write(wt_a.join("f.txt"), "worktree edit\n").unwrap();
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        assert!(
            cx.read(|app| kagi.read(app).commit_panel_is_foreign(app)),
            "the worktree panel must be up — that is the trap the origin avoids"
        );

        // The real handler behind the tree's "Discard Changes…" menu item.
        let discard_f = EditorTreeAction::Discard(PathBuf::from("f.txt"));
        win.update(cx, |_, window, app_cx| {
            kagi.update(app_cx, |app, cx| {
                app.dispatch_editor_tree_action(discard_f, window, cx)
            })
        })
        .expect("dispatch editor-tree discard");
        cx.run_until_parked();
        kagi.update(cx, |app, cx| app.start_discard(cx));
        cx.run_until_parked();

        assert_eq!(
            repo_fingerprint(&repo_path),
            repo_fp,
            "an editor-tree discard must restore the OPEN repo's f.txt (back to the \
             fixture state: only the untracked dirty.txt left)"
        );
        assert_eq!(
            repo_fingerprint(&wt_a).1,
            " M f.txt\n",
            "an editor-tree discard must NOT touch the worktree's copy of the same path"
        );

        // …and the panel's own discard still goes to the worktree.
        let path_a = wt_a.clone();
        kagi.update(cx, |app, cx| {
            e2e::open_worktree_panel_no_inputs(app, path_a, "wt-a", idx_a, cx)
        });
        cx.run_until_parked();
        kagi.update(cx, |app, cx| {
            app.open_discard_all_modal(app.active_session().unwrap(), cx)
        });
        cx.run_until_parked();
        kagi.update(cx, |app, cx| app.start_discard(cx));
        cx.run_until_parked();
        assert_eq!(
            repo_fingerprint(&wt_a),
            (head_after_amend.clone(), String::new()),
            "the panel's own discard must clean the WORKTREE"
        );
        assert_eq!(
            repo_fingerprint(&repo_path),
            repo_fp,
            "the panel's discard must not touch the open repo"
        );

        unmount(cx, kagi, win);
        eprintln!(
            "[gui-e2e] PASS worktree_panel_amend_discard wt-a {}..{} backup={} undo={undo_head:?}",
            &head_before[..8],
            &head_after_amend[..8],
            &blob[..8],
        );
    }

    /// Real lock timeout after mutation: preserve recovery and deliver the notice
    /// even if the owner generation changes before the background result arrives.
    fn scenario_worktree_panel_discard_recording_failure(cx: &mut VisualTestAppContext) {
        for stale in [false, true] {
            let (_fixture, repo_path, wt_a, wt_b) = build_two_worktree_fixture();
            let original = std::fs::read(wt_a.join("dirty.txt")).unwrap();
            let foreign_before = repo_fingerprint(&wt_b);
            let (kagi, win) = mount(cx, &repo_path);
            let index = cx.read(|app| {
                kagi.read(app)
                    .view()
                    .worktrees
                    .iter()
                    .position(|wt| wt.path == wt_a)
                    .unwrap()
            });
            kagi.update(cx, |app, cx| {
                app.set_window_active_for_e2e(true, cx);
                e2e::open_worktree_panel_no_inputs(app, wt_a.clone(), "wt-a", index, cx)
            });
            cx.run_until_parked();
            kagi.update(cx, |app, cx| {
                app.open_discard_all_modal(app.active_session().unwrap(), cx)
            });
            cx.run_until_parked();
            let log_dir = PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap());
            let log_path = log_dir.join("operations.jsonl");
            let before_log = std::fs::read(&log_path).unwrap_or_default();
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(log_dir.join("operations.jsonl.lock"))
                .unwrap();
            let presentation = |cx: &mut VisualTestAppContext| {
                cx.read(|app| {
                    let state = kagi.read(app);
                    (
                        format!("{:?}", state.status_footer),
                        (state.bottom_panel_open, state.bottom_tab),
                    )
                })
            };
            let mut presentation_before = presentation(cx);
            let (hold, release) = crate::evidence_support::deferred::<()>(cx);
            kagi::ui::KagiApp::hold_next_run_for_e2e(hold);
            kagi.update(cx, |app, cx| app.start_discard(cx));
            cx.run_until_parked();
            cx.read(|app| assert!(kagi.read(app).app_sessions.has_leases()));
            assert_eq!(std::fs::read(wt_a.join("dirty.txt")).unwrap(), original);
            if stale {
                // Change the actual consumer context while the admitted run
                // remains held, instead of retiring an otherwise-active visit.
                let foreign = wt_b.canonicalize().unwrap();
                kagi.update(cx, |app, cx| {
                    assert!(app.open_repository(foreign.clone(), cx))
                });
                cx.run_until_parked();
                cx.read(|app| {
                    assert_eq!(kagi.read(app).repo_path.as_ref(), Some(&foreign));
                });
                presentation_before = presentation(cx);
            }
            lock.lock().unwrap();
            release.send(());
            cx.run_until_parked();
            drop(lock);
            assert_eq!(
                repo_fingerprint(&wt_a).1,
                "",
                "mutation must already have completed"
            );
            assert_eq!(
                repo_fingerprint(&wt_b),
                foreign_before,
                "the foreign worktree is not a target of the frozen discard"
            );
            assert_eq!(
                std::fs::read(&log_path).unwrap_or_default(),
                before_log,
                "no fake durable success or retry"
            );
            cx.read(|app| {
                let state = kagi.read(app);
                let notice =
                    e2e::app_notice_message(state).expect("owner-named recording failure notice");
                let append_diagnostic = std::fs::TryLockError::WouldBlock.to_string();
                assert!(
                    notice.contains(wt_a.to_str().unwrap()) && notice.contains(&append_diagnostic),
                    "{notice}"
                );
                assert!(
                    !e2e::queued_notice_contains(state, wt_a.to_str().unwrap()),
                    "one failed recording must not queue another owner-named acknowledgment"
                );
                assert!(state
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(app)
                    .toasts()
                    .is_empty());
                assert_eq!(
                    (state.bottom_panel_open, state.bottom_tab),
                    presentation_before.1,
                    "only explicit inspection may switch the existing panel"
                );
                if stale {
                    assert_eq!(format!("{:?}", state.status_footer), presentation_before.0);
                } else {
                    assert!(
                        matches!(
                            &state.status_footer,
                            kagi::ui::types::FooterStatus::Idle(message) if message.is_empty()
                        ),
                        "required acknowledgment is the only result: {:?}",
                        state.status_footer
                    );
                }
                assert!(
                    state.discard_modal().is_none(),
                    "must not offer to repeat a completed mutation"
                );
                if !stale {
                    let panel = state.op_log.as_ref().unwrap().read(app);
                    let entry = panel.entries().front().unwrap();
                    let kagi_git::oplog::OpOutcome::Partial { error, .. } = &entry.outcome else {
                        panic!("unrecorded discard must remain an attempted partial row")
                    };
                    assert!(error.contains(&append_diagnostic), "{entry:?}");
                    assert_eq!(entry.backup_refs.len(), 1);
                    let output = Command::new("git")
                        .current_dir(&wt_a)
                        .args([
                            "cat-file",
                            "blob",
                            &format!("{}:file", entry.backup_refs[0]),
                        ])
                        .output()
                        .unwrap();
                    assert!(output.status.success());
                    assert_eq!(output.stdout, original);
                    let tree = Command::new("git")
                        .current_dir(&wt_a)
                        .args(["ls-tree", &entry.backup_refs[0]])
                        .output()
                        .unwrap();
                    assert!(tree.status.success());
                    let blob = rev_parse(&wt_a, &format!("{}:file", entry.backup_refs[0]));
                    assert_eq!(
                        tree.stdout,
                        format!("100644 blob {blob}\tfile\n").as_bytes()
                    );
                }
            });
            kagi.update(cx, |app, cx| app.confirm_app_notice(cx));
            cx.run_until_parked();
            cx.read(|app| {
                let state = kagi.read(app);
                assert!(e2e::app_notice_message(state).is_none());
                assert!(!e2e::queued_notice_contains(state, wt_a.to_str().unwrap()));
            });
            assert_eq!(
                std::fs::read(&log_path).unwrap_or_default(),
                before_log,
                "acknowledging lost recording must not replay or invent a receipt"
            );
            unmount(cx, kagi, win);
        }
        eprintln!("[gui-e2e] PASS worktree_panel_discard_recording_failure current + stale owner");
    }

    /// Try to capture a native Metal PNG. Capture remains best-effort for
    /// platforms without image support; successful frames must be non-blank
    /// and are saved to `$CARGO_TARGET_DIR/gui_e2e_poc/<tag>.png`.
    pub(super) fn capture_screenshot_best_effort(
        cx: &mut VisualTestAppContext,
        win: gpui::AnyWindowHandle,
        tag: &str,
    ) {
        match cx.capture_screenshot(win) {
            Ok(img) => {
                assert!(
                    img.width() > 0 && img.height() > 0,
                    "{tag} screenshot empty"
                );
                assert!(
                    img.pixels().any(|p| p.0 != [255, 255, 255, 255]),
                    "{tag} screenshot is a blank white frame"
                );
                let path = out_dir().join(format!("{tag}.png"));
                img.save(&path).expect("save png");
                eprintln!("[gui-e2e] screenshot {tag}: {}", path.display());
            }
            Err(e) => eprintln!("[gui-e2e] screenshot {tag}: skipped (native capture failed: {e})"),
        }
    }
}
