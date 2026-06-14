# T-MAINSLIM-001: Slim down src/main.rs — extract the KAGI_* headless harness

- Status: todo
- Group: アーキテクチャ / bin shell
- 仕様の正: ADR-0077 (retire/relocate KAGI_*), architecture.md §2.5 (thin bin shell).

## 背景(調査済み)
- `src/main.rs` = **1457 LOC**。うち大半が **KAGI_* headless テストハーネス**(146 KAGI_ 参照、
  `record_headless_op` / `run_headless_discard` / `init_tab` + ~47 の env-var 駆動フロー)。
- OpenLogi の `main.rs` は 512 LOC(薄い shell)。
- ADR-0077 は KAGI_* の退役/縮小を方針化済み。本チケットは**第一歩=ハーネスを別モジュールへ抽出**し、
  `main.rs` を「window 作成 + 実アプリ起動 + 必要最小の bootstrap」に薄くする(挙動は不変)。

## スコープ(純粋な移設、挙動不変)
- 新規 `src/headless.rs`(または `src/ui/headless.rs`)に headless 関連を移動:
  `record_headless_op`、`run_headless_discard`、`init_tab`、および main() 内の全 `KAGI_*` 分岐ロジック
  (KAGI_OPEN_REPO / MENU_DUMP / SELECT_FIRST / JUMP / CONTEXT_MENU / COMPARE_* / OPEN_FIRST_FILE /
  PULL / PUSH / CHECKOUT / COMMIT / AMEND / DISCARD / STASH_* / CHERRY_PICK / REVERT / UNDO / POP /
  CREATE_BRANCH / DELETE_BRANCH / TERMINAL / BOTTOM_PANEL / COMMIT_PANEL / PLAN_* など)。
- `main()` からは **単一の呼び出し**(例 `headless::run_if_requested(&mut app_state, ...) -> bool`)で
  headless 経路へ委譲。theme/lang の初期化や window 起動など「実アプリにも必要」な部分は main に残す。
- **stderr のログ文字列(`[kagi] ...`)を一字一句変えない**(既存の env ハーネステストが grep している)。
- env var の優先順位・挙動を変えない。`KAGI_AUTO_CONFIRM` 等の意味も不変。

## 完了条件(受け入れ条件)
- [ ] `src/main.rs` が大幅に縮小(目標 < 400 LOC、OpenLogi 水準)。headless ロジックは新モジュールへ。
- [ ] `main()` は薄い:env 初期化 → headless 委譲(1 呼び出し)→ window/app 起動。
- [ ] すべての `KAGI_*` 経路が以前と同一挙動・同一 stderr ログを出す(回帰なし)。
- [ ] 通常起動(env なし)が以前どおり動く(GUI が出る)。
- [ ] `cargo test --workspace` 全パス + `grep -rE 'git2::|Repository::open' src/ui` = 0(main.rs の git2 は対象外だが新モジュールに git ロジックを増やさない)。

## 規約
- 純粋な移設。ロジック・文字列・順序を変えない。新機能を足さない。
- fixture/tempdir のみで検証。

## やってはいけないこと
ログ文字列の変更 / env 挙動の変更 / headless 経路の削除(retire は別チケット)/ 移設ついでの挙動変更。

## Implementation memo

- Status: done (PM merged + smoke-tested 2026-06-14)
- 純粋な移設 (pure relocation): 文字列・env名・優先順位・順序を一字一句保持。

### 移設内容
新規 `src/headless.rs`(`mod headless;` を main.rs に追加)へ移動:
- `record_headless_op`(`pub(crate)`)、`init_tab`(`pub(crate)`)、`run_headless_discard`(private)。
- main() 内の全 `KAGI_*` 分岐ロジック(SELECT_FIRST / JUMP / CONTEXT_MENU / COMPARE_HEAD/WT /
  OPEN_FIRST_FILE / PULL / PUSH / UNDO / POP / DISCARD(_ALL) / AMEND / DELETE_BRANCH /
  PLAN_CHECKOUT / CHECKOUT_COMMIT / CREATE_BRANCH / PLAN_WORKTREE / STASH_PUSH/APPLY /
  CHERRY_PICK / REVERT / COMMIT_PANEL(+STAGE/UNSTAGE/COMMIT_MSG)/ COMPACT / BOTTOM_PANEL /
  TERMINAL / MENU_DUMP)を verbatim でコピー。

### main() が呼ぶ単一エントリポイント
- repo-path 起動経路: `pub fn run_repo_flow(app_state: KagiApp, repo_path: PathBuf, env_open_repo: Option<PathBuf>)`
  — `app_state` を **所有**して全 KAGI_* フックを適用し、最後に `run_app(app_state)` を呼ぶ。
  これにより元 main() にあった「途中の `run_app(app_state); return;` 早期終了経路」を完全保持。
- no-arg(Welcome)経路: `pub fn run_welcome_hooks(welcome: &mut KagiApp, env_open_repo: &Option<PathBuf>)`
  — `KAGI_OPEN_REPO`(init_tab)と `KAGI_MENU_DUMP` を適用。welcome 構築/session restore は main() に残置。
- main() に残るもの: theme/zoom/compact/lang の env 初期化、usage/Welcome 構築、repo open + snapshot +
  stderr 診断、初期 single-tab `app_state` 構築、headless 委譲(1 呼び出し)。

### LOC
- `src/main.rs`: 1461 → **149**(目標 <400 達成)。
- `src/headless.rs`: **1363**(新規)。

### 検証
- `cargo build`: 0 warnings(unused import なし)。
- `cargo test --workspace`: **666 passed; 0 failed**(全 KAGI_* binary-spawning 結合テスト含む:
  revert/push/compare/undo_redo/branch_sync/conflicts 等が green → stderr 文字列 byte-identical を確認)。
- `grep -rnE 'git2::|Repository::open' src/ui/` = 0(不変)。headless.rs の git2 は移設分のみ、新規ロジックなし。

### 注意 / 違和感のあった点
- 早期 `run_app(app_state); return;` を持つ KAGI_* 経路(PLAN_WORKTREE の不正 spec、STASH_PUSH/APPLY/
  CHERRY_PICK/REVERT/COMMIT verify の repo open エラー等)が、`app_state` を所有する `run_repo_flow` 形に
  したことで自然に保持できた。bool を返す形だと所有権の都合で早期 `run_app` を表現しづらかったため、
  「app_state を渡して module 側で run_app まで責務を持つ」設計を採用(ティケット許容範囲)。
- `CreateBranchModal` リテラルの既存インデント崩れ(`input_state: None,` の 1 行)は verbatim 維持。
