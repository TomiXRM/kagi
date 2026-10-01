# ADR-0208: embedded-terminal cwd auto-lock — Phase 1（所有権と観測の土台）

- Status: **Accepted**（Phase 1 のみ。所有権と tab 配送の安全修正は [ADR-0218](0218-terminal-autolock-owner-confirmation.md)。Phase 2 = 自動 acquire の confirm 省略・複数 terminal の集約・release の自動化は本 ADR の範囲外）
- Date: 2026-10-01
- Related: [#772](https://github.com/TomiXRM/kagi/issues/772)（契約 A–D、欠落 1–3）、#372 item 3、
  ADR-0035（worktree lifecycle）、ADR-0177（`record_op_persist` の同期 UI 例外 — lock / unlock）、
  ADR-0178 / ADR-0182（session identity / close は実行取消ではない）、ADR-0188（subprocess runner）、
  ADR-0196（lifecycle 契約）、ADR-0197（session-owned UI state）

## 0. アーキテクチャレビュー（5 点）

1. **SEARCH.** lock / unlock は既に `plan_/execute_` 三つ組で存在する:
   `crates/kagi-git/src/ops/worktree_lifecycle.rs:163-248`（`plan_lock_worktree(repo, name, reason)` /
   `execute_lock_worktree`）、`crates/kagi-git/src/ops/worktree.rs:611-723`（`plan_unlock_worktree` /
   `execute_unlock_worktree`）。UI は `src/ui/operations/worktree_lock.rs`（`LockWorktreeModal` /
   `UnlockWorktreeModal`、`record_op_persist("lock-worktree" | "unlock-worktree")`）。
   terminal は `src/ui/terminal.rs:206-318` `build_terminal_view` が `spawn_command` の `Child` を捨てている
   （欠落 1）。exit は `with_exit_callback`（PTY EOF、render 時配送）だけ（欠落 2）。unlock は `Locked(_)`
   しか見ない（欠落 3）。PID/cwd probe は `crates/kagi-git/src/proc/` に無い（`group_alive` は pgid の生存のみ）。
   worktree identity は `kagi_domain::remove::WorktreeId { repo: RepoId, git_dir }`（`Backend::write_worktree_id`）。
2. **OWNER.** lock 状態の真実 = git の worktree admin file（`git2::Worktree::is_locked/lock/unlock`）、
   その変更 = `ops/worktree_lifecycle.rs` の三つ組。子プロセスの所有 = `KagiTerminalSession`（`TabUiState`、
   session-owned、`release_session` で破棄）。cwd probe の OS 境界 = `kagi_git::proc`。設定 = `kagi-ui-core`
   `Settings` + `theme.rs` の atomic（`auto_fetch` と同型）。
3. **DECISION.** EXTEND:
   - token の判定は **pure** なので `kagi-domain` に `worktree_autolock` を足す（新 module だが新 crate / manager ではない。
     `kagi-domain` は依存ゼロのまま）。
   - 自動 acquire は **既存 `plan_lock_worktree` をそのまま使う**（reason に token を渡すだけ）。
   - 自動 release は既存の手動 unlock を転用せず、同じ file に `plan_/preflight_/execute_auto_unlock_worktree` を足す
     （契約 B: 手動 lock に触れない判定を preflight に持つ）。
   - UI の release 確認は `UnlockWorktreeModal` に `auto: Option<AutoLockOffer>`（owner / 世代 / 凍結 path / `AutoUnlockTarget`）を足して同じ card / 同じ
     `confirm_unlock_worktree` から分岐する（modal 変種を増やさない。ADR-0093 の 1 slot はそのまま）。所有権の確定時点は ADR-0218。
   - `KagiTerminalSession` に `shell: Option<ShellProcess>`（PID・spawn 世代・wait 結果）を足す。
   - cwd probe は `kagi_git::proc::cwd_of_pid`（macOS `proc_pidinfo(PROC_PIDVNODEPATHINFO)` / Linux `/proc/<pid>/cwd`、
     Windows は `Unsupported`）。
4. **REFACTOR.** なし。`lock_reason(wt)`（`worktree_lifecycle.rs:145`）を auto_unlock でも再利用する。
5. **ANTI-PATTERNS.** #1（manager）: 状態は `KagiTerminalSession` に置き、`release_session` が破棄する。
   #2（UI が domain を複製）: lock の有無は毎回 git から読み、UI に「locked」のコピーを持たない。
   #3（render 内 I/O）: acquire の plan は terminal 起動時の event handler で `Backend` を開く既存
   `worktree_backend` 経由、release の plan は background の wait 配送後に行う。#5（flag 分岐）: 自動 release は
   `UnlockWorktreeModal.auto` の有無で同じ経路を分岐させ、並行 modal を作らない。#7（同じ問題を別名で）: lock の
   plan は 1 本のまま。

## 文脈

`git worktree lock` は「この worktree を消すな」という人の意思表示で、Kagi の terminal がそこで作業している間だけ
自動で立てたい（#372 item 3）。契約 A–D（#772）は承認済み: macOS / Linux 先行、既定 OFF の opt-in、各変更は
plan → confirm → preflight → execute → verify → oplog、**Kagi 所有 token 付き lock 以外に触れない**、観測不能は
退出扱いにしない、crash 後の残骸は既存の確認付き手動解除で回復する。

Phase 1 が埋めるのは調査で判明した 3 つの欠落だけである。

## 決定

1. **Kagi 所有 token（pure、`kagi_domain::worktree_autolock`）。**
   - reason の形式は `kagi:auto:<owner>`。`owner` は 1 terminal session の識別子（空でなく、空白・改行を含まない）。
   - `AutoLockToken::parse(reason)` は **この形式に完全一致する reason だけ**を token として返す。手動 lock の reason
     （既定文言 "locked in kagi" を含む）、prefix だけの reason、空 reason は `None`。
   - `classify_auto_unlock(found_reason, expected_token, found_identity, expected_identity)` が release の可否を
     `Result<(), AutoUnlockRefusal>` で返す: `NotLocked` / `NotAutoLock`（手動・他プロセス）/ `TokenMismatch`
     （他 session の Kagi lock）/ `IdentityMismatch`（対象 worktree が想定と違う）。
2. **自動 release 専用の三つ組（`ops/worktree_lifecycle.rs`）。**
   - `plan_auto_unlock_worktree(repo, name, expected: &AutoUnlockTarget)`: lock を読み、決定 1 で分類。
     拒否は blocker `WorktreeNote::AutoUnlockRefused { name, refusal }`。
   - `preflight_auto_unlock_worktree`: `preflight_check`（HEAD 不変）に加えて **reason と identity を再読して
     同じ分類を通す**（欠落 3）。`execute_auto_unlock_worktree` は preflight → `wt.unlock()` → verify。
   - **未解決のまま明示する競合**: read-then-unlock は atomic ではない。外部 git が同じ瞬間に unlock → 別 reason で relock
     した場合、preflight 直後の relock を Kagi が消しうる。Phase 1 は窓を「preflight 直後の 1 回の `unlock`」まで
     狭めるが、これを「手動 lock に触れない」の達成とは**主張しない**（#772「実装ゲート」）。Phase 2 で
     `.git/worktrees/<name>/locked` の内容比較付き削除（open → 再読 → unlink、または rename-then-verify）を検討する。
   - 既存の手動 `plan_unlock_worktree` / `execute_unlock_worktree` は無変更。
3. **Child の所有と終了観測（`src/ui/terminal.rs`）。**
   - `build_terminal_view` は `Child` を捨てず、`ShellProcess { pid, generation }` を `KagiTerminalSession.shell` に置く。
     `generation` は session ごとの spawn 世代（再起動で +1）。
   - 終了観測は render 非依存: `cx.background_spawn` で `child.wait()` を待つ task が結果を `this.update` で配送し、
     `owner`（`SessionId`）と `generation` が一致するときだけ `ShellProcess.exit` に書く。非表示 terminal でも届く。
     `wait` の `Err` は `ShellExit::Unknown` として保持し、**終了扱いにしない**（契約 C の精神）。
     契約行 `[kagi] terminal: shell exited` は従来どおり PTY EOF callback から出し、新たに
     `[kagi] terminal: shell wait: gen=<n> status=<exited <code>|unknown>` を wait 配送で出す。
   - cwd 観測 `kagi_git::proc::cwd_of_pid(pid) -> Result<PathBuf, CwdProbe>`。失敗は `CwdProbe::Unknown(..)` /
     `Unsupported`（Windows）で、呼び手は「不明」を保持する。Phase 1 では `KagiTerminalSession::observe_cwd()` を
     公開し、終了・退出の判定には使わない。
4. **設定（既定 OFF）。** `settings.json` の `"terminal_auto_lock": "true"|"false"`、`Settings::terminal_auto_lock()`、
   `theme::terminal_auto_lock()` / `set_terminal_auto_lock`、Settings window に toggle 1 つ（EN/JA）。
   ON のとき terminal を起動した tab が **linked worktree** なら、起動直後に `plan_lock_worktree(name, kagi:auto:<owner>)`
   の plan card（既存 `LockWorktreeModal`）を開く。**confirm するまで lock は立たない**。すでに locked（手動でも Kagi でも）
   なら plan は既存の `AlreadyLocked` blocker になり、何も上書きしない。shell の終了が配送され、その lock がこの session の
   token なら `auto` 付きの `UnlockWorktreeModal`（決定 2）を開く。confirm 省略の可否は Phase 2。
5. **crash（契約 D）。** 残った `kagi:auto:*` lock は自動回収しない。既存の手動 unlock modal がその reason を警告
   として見せるので、ユーザーが確認して解除する。

## 却下した案

- **手動 unlock を自動 release に転用**: reason / identity を見ないので契約 B に反する（欠落 3）。
- **terminal title / 起動 cwd を現在 cwd の代用にする**: 契約 A で禁止。Windows は `Unsupported` を返し、成功扱いにしない。
- **exit callback（PTY EOF）を終了証拠にする**: render 時配送で、非表示 terminal では届かない（欠落 2）。
- **prefix / PID だけで残骸を自動回収**: 契約 D。PID は再利用されるし、prefix は他プロセスの Kagi かもしれない。
- **vendor（gpui-terminal）に lock policy を置く**: #772「vendor に Kagi 固有 policy を作らない」。

## 結果

- `kagi-domain` に pure module 1 つ（依存追加なし）。`WorktreeNote` に `AutoUnlockRefused`、`WorktreeTitle` に
  `AutoUnlockWorktree` が増える。
- `kagi-git` に `cwd_of_pid` と auto_unlock 三つ組。統合テストで「手動 lock を触らない」「token 不一致で拒否」
  「identity 不一致で拒否」を固定する。
- 既定 OFF なので既存ユーザーの挙動は変わらない。ON でも confirm 前には何も書かない。
- ADR-0177 の `record_op_persist` 例外表の lock / unlock 行はそのまま（新しい writer は増えない: 自動 release も
  `confirm_unlock_worktree` の同じ writer を通る）。
