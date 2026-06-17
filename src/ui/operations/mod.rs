//! Per-operation orchestration for `KagiApp`, split out of `ui/mod.rs`
//! (issue #13 Phase 4, P1). Each submodule holds the `open_/cancel_/replan_/
//! confirm_/start_` methods (plus async/finish helpers) for one family of Git
//! operations as additional `impl KagiApp` blocks. Pure physical split —
//! behaviour and signatures are unchanged.

pub mod branch;
pub mod checkout;
pub mod cherry_revert;
pub mod commit;
pub mod conflict;
pub mod discard;
pub mod history;
pub mod pull_push;
pub mod stash;
pub mod worktree;
