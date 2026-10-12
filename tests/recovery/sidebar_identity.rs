//! #1303–#1305: sidebar identity, read-only activation and narrow group headers.
//! Native renderer/input contracts, not native VoiceOver or foreground evidence.
#[path = "sidebar_identity/groups.rs"]
mod groups;
#[path = "sidebar_identity/stash.rs"]
mod stash;
#[path = "sidebar_identity/support.rs"]
mod support;
#[path = "sidebar_identity/tag.rs"]
mod tag;

pub use groups::{scenario_remote_group_header_fit, scenario_remote_group_header_fit_cell};
pub use stash::scenario_stash_row_identity;
pub use tag::{scenario_tag_unloaded_target, scenario_tag_unloaded_target_cell};
