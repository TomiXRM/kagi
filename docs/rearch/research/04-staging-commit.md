# 04 — Staging / Commit / Amend / Templates / Drafts / Checklist / Smart Commit

Re-architecture research (sub-agent #4). Scope: staging, commit, amend, message
template, draft autosave, pre-commit checklist, smart commit messages.

Layering target: **domain** (pure) → **git-backend** (operations in the safety
pipeline) → **app** (AppState commit-draft + async tasks) → **ui** (view-model +
view). Invariants: UI never calls git2 directly; the Ollama LLM stays strictly
opt-in / localhost / staged-diff-only.

---

## 1. Kagi 現状

### 1.1 git-backend (`src/git/`)
- **`staging.rs`** — `stage_file` / `unstage_file` / `stage_files` / `unstage_files`
  (index-only, WT 不変が doc + test 不変条件); `unstaged_file_diff` /
  `staged_file_diff` (Patch→`FileDiff`); `commit_preview` (pure read: A/M/D 集計
  + target branch + author, 例外なし `(unknown)` fallback); `plan_commit`
  (blocker: 空 message / staged 空 / conflict 状態; warning: leftover; +
  `checklist()` を畳み込み) → `OperationPlan`; `execute_commit` (index→tree→
  `repo.commit`, unborn/normal 分岐, WT 不変).
- **`checklist.rs`** — pure `checklist(repo, status) -> (blockers, warnings)`。
  ADR-0043 rules 4/5/6 を **index BLOB のみ**走査 (conflict marker=block、
  secret/.env=warn、large binary=warn)。`text_has_conflict_marker` を
  conflict-resolution buffer (ADR-0057) と共有。bounded scan (1MiB / 8KiB /
  NUL probe)。env override `KAGI_LARGE_BLOB_BYTES`。
- **`message_template.rs`** — 完全 pure。`TemplateFields{type,scope,summary,body,
  test,risk}` + `assemble()` (Conventional Commits 風、空フィールド省略、
  Test:/Risk: trailer) + `parse_message()` (plain⇄template 往復、lossless
  fallback)。`TYPE_CHOICES` 定数。
- **`drafts.rs`** — `Draft{repo,branch,message,mode,updated}` +
  `save_draft`/`load_draft`/`clear_draft`。`$KAGI_LOG_DIR/drafts/` or
  `$HOME/.kagi/drafts/`、ファイル名 = `sha1(repo\0branch).json`、手書き JSON
  (serde 禁止、oplog escaping 流用)、自前 SHA-1。空 message は削除。寛容 read
  (壊れ draft = 無視)。
- **`message_gen.rs`** — Smart Commit backend。`MessageBackend::{Ollama,RuleBased}`
  enum dispatch (trait なし)。`collect_staged_files` / `collect_staged_diff`
  (HEAD→index = staged のみ、~8KB truncate + file summary)。`rule_based()`
  (pure、決定的、必ず非空、type 推論 + scope 推論 + lang/style)。`Ollama`:
  `POST localhost:11434/api/generate`, ureq 3 再利用、手書き JSON parse、
  `KAGI_OFFLINE=1` で停止。`ollama_available`/`ollama_list_models`
  (reachability + `/api/tags`、diff は送らない)。失敗時は caller が rule-based
  へ静かに fallback。
- **`trailers.rs`** — pure `parse_coauthors()` (Co-authored-by: trailer、
  UTF-8/CJK safe、dedup)。
- **amend / undo** — `ops.rs`: `plan_amend`/`execute_amend`/`AmendMode{
  MessageOnly,Staged,Both}`/`AmendOutcome`。**`commit.amend` を使わず** new
  commit + ref 移動 (ADR-0040、ref-order 規則、author 保持、checklist 通す、旧
  HEAD を oplog 記録、pushed amend = blocker)。`plan_undo_commit`/
  `execute_undo_commit` (soft 相当、ref のみ戻す、WT/index 不変、ADR-0041)。

### 1.2 ui (`src/ui/`)
- **`commit_panel.rs`** — `CommitPanelState{unstaged,staged,*_stats,
  conflicted_paths,selected_file,commit_msg,plan_modal,tree_view}`。
  `from_repo`/`reload_status` が **git2 を直接開く** (`Repository::open` +
  `working_tree_status` + diffstat)。`CommitPlanModal`、`status_badge`。
- **`smart_commit.rs`** — `SmartCommitState{ollama_available,detected_models,
  llm_enabled,model,lang,style,modal,generating,status}` + settings.json 永続化
  (`smart_commit_*` keys)、`SmartCommitModal::{Consent,ModelPicker}`、
  `CONSENT_LINES` (ADR-0044 の 4 文言を verbatim 保持 + test)、`llm_offered()`。
- **`mod.rs` (KagiApp)** — commit-draft 状態がすべてここに散在:
  `commit_panel_open`, `commit_panel: Option<CommitPanelState>`,
  `commit_input: Option<Entity<InputState>>` (gpui-component, IME 対応),
  `commit_template_mode`, `commit_template_inputs: Option<[Entity<InputState>;6]>`,
  `smart_commit`, `smart_commit_detected_for`, `pending_smart_msg`,
  `last_draft_value`, `draft_save_gen` (debounce 世代), `amend_modal`。
  - `open_commit_panel` — InputState lazy 生成 → `from_repo` → **`load_draft`**
    (空 input のみ、mode で plain/template 復元) → focus → `ensure_smart_commit_detection`。
  - draft autosave — `sync_modal_inputs` 系で input 値変化を検知 → `draft_save_gen`
    インクリメント → `background_spawn` で `save_draft`/`clear_draft` (最新世代のみ実行 = debounce)。
  - smart-commit — `ensure_smart_commit_detection` (background reachability) /
    `run_smart_generation` (background `collect_staged_diff` + `generate_message`、
    結果は `pending_smart_msg` 経由で次 render の Window で Input へ流す)。
  - amend — `open_amend_modal`/`confirm_amend` (2段階 armed 確認、
    `plan_amend`/`execute_amend`)。

### 1.3 現状の構造的問題 (re-arch で直すべき点)
1. **UI が git2 を直接呼ぶ**: `commit_panel.rs::reload_status` と `mod.rs` の各所が
   `git2::Repository::open` を直接実行 → 不変条件「UI never calls git2」違反。
2. **commit-draft 状態が KagiApp に肥大化**: 10+ フィールドが平坦に散在し、commit
   関連の状態境界が無い。view-model に括れていない。
3. **`Entity<InputState>` への依存が値読み取りに混入**: draft / smart-commit /
   amend が「Input の現在値」を読むために `cx` + Entity を直接触る。pure な
   commit-draft モデルと UI widget が結合。
4. **背景タスクの戻り値配線が ad-hoc**: `pending_smart_msg` は「Window が無いので
   次 render で流す」苦肉の策。app 層の明確な command/result チャネルが無い。
5. **plan/checklist の重複**: rule 1–3 が `plan_commit` 内、rule 4–6 が
   `checklist.rs` に分かれている (ADR 上は意図的だが、re-arch では 1 つの
   `evaluate_commit(plan_input) -> Verdict` に統合余地)。

---

## 2. 参考プロジェクトの実装方針

### 2.1 Zed git panel (crates/git_ui)
- commit message は専用 `Editor` entity (multi-line)。staging/commit は
  `Repository`/`GitStore` という **backend service entity** 経由で、panel view は
  git2/CLI を直接触らない。`commit`/`stage`/`unstage` は async action を
  store に dispatch → store がワーカで実行 → イベントで panel 更新。
- 採用すべき発想: **「view → command → backend service → event → view」の単方向
  フロー**。commit message editor は 1 つの owned entity で、view-model がその
  値を読むだけ。

### 2.2 gpui-component Input / TextArea / Form (`docs/research/gpui-component-audit.md`)
- `Input`/`InputState` は **採用済** (IME 対応)。multi-line は `InputState` の
  `code_editor`/text モード (conflict editor が `InputState::new(...).code_editor`
  で利用中)。`form/`(`Form`/`Field`) は「フォームレイアウト」用途で S 評価
  (template の 6 フィールド整列に候補)。
- 採用方針: **commit message = IME 対応 `InputState` のまま** (T014/T026 で確立)。
  template モードの 6 フィールドは `[Entity<InputState>;6]` を `Form`/`Field`
  レイアウトに載せる。view-model は InputState entity を **保持はするが値の
  source-of-truth にはしない** (下記 §3.2)。

### 2.3 GitButler (`docs/research/gitbutler-reuse-research.md`)
- **コード流用不可** (FSL、concept adoption のみ)。`but-action` (worktree 変更→
  自動コミット)、`but-llm` (OpenAI/Anthropic/Ollama/LM Studio 統合)、`but-rules`
  (宣言的 automation) はいずれも **MVP スコープ外 / Study only**。
- 採用しない: virtual branch / workspace commit / AI auto-commit / 外部 LLM 統合。
  Kagi の安全パイプライン + localhost-only LLM 方針と思想衝突。

---

## 3. 採用すべき設計

### 3.1 staging/commit ops のパイプライン上の位置
- **domain (pure)**: template parse/assemble、checklist rules、message-gen rules、
  trailer parse、`CommitPreview` 集計ロジック。git2 非依存、入力は plain struct。
- **git-backend (operation)**: `stage_file(s)` / `unstage_file(s)` /
  `plan_commit` / `execute_commit` / `plan_amend` / `execute_amend` /
  `plan_undo_commit` / `execute_undo_commit` を **既存の plan→confirm→execute
  パイプラインの operation として配置**。すべて `(repo, input) -> Result` の純
  関数 (現状維持で良い、ファイル移動のみ)。`checklist()` を `plan_commit`/
  `plan_amend` 双方が呼ぶ (ADR-0043 §Consequences)。fixup/squash は
  `execute_commit` の message 組み立てだけで実現 (ADR-0045、新 pipeline 不要)。
- **app**: AppState が repo handle を 1 箇所で所有し、UI からの commit/stage
  command を受けて backend operation を呼ぶ。**UI は git2 を一切開かない** →
  `commit_panel.rs::reload_status` の `Repository::open` を app 層の
  `RepoService`/`AppState::reload_staging()` へ移す。
- **ui**: command を発行し、結果イベントで view-model を更新するだけ。

### 3.2 commit-draft 状態の所有 (ownership)
- KagiApp に散る 10+ フィールドを **`CommitDraft` view-model 1 つに集約**:
  ```
  CommitDraft {
    mode: PlainOrTemplate,
    plain: String,                 // source of truth (plain)
    template: TemplateFields,      // source of truth (template)
    smart: SmartCommitState,
    autosave: DraftAutosave { last_value, gen },
    plan_modal / amend_modal,
  }
  ```
- **`InputState` entity は view が保持し、view-model の文字列と双方向同期** する。
  値の source-of-truth は `CommitDraft` 側の plain/`TemplateFields`。これにより
  draft 保存・smart-commit 結果反映・plan 構築が `cx`/Entity を触らず純データで
  完結し、`pending_smart_msg` のような苦肉策が不要になる (command result を
  view-model に書く → 次 render が InputState へ反映)。
- mode 切替は `assemble`/`parse_message` で plain⇄template を lossless 変換。

### 3.3 template / checklist を pure domain として
- `message_template.rs` は既に完全 pure → **そのまま domain へ移設**。view は
  6 `InputState` を `TemplateFields` に詰め、`assemble()` の結果を commit に渡す
  だけ。
- `checklist.rs` も pure (git2 read はするが mutation/UI/oplog 無し) → domain
  寄り backend ヘルパとして維持。re-arch では **commit/amend 共通の
  `evaluate_commit(repo, status, message) -> CommitVerdict{blockers,warnings}`**
  に rule 1–6 を集約し、`plan_commit`/`plan_amend` がそれを呼ぶ形に整理 (現状の
  「rule 1–3 は plan 内 / 4–6 は checklist」の二分を解消)。block/warn 分類は
  ADR-0039、override は warn のみ 1 クリック + oplog note (ADR-0039/0043)。

### 3.4 draft autosave の永続化
- `drafts.rs` の API (save/load/clear、手書き JSON、`~/.kagi/drafts/`、
  `sha1(repo\0branch)` key) を **そのまま維持** (ADR-0042 準拠、oplog/avatar 流儀
  に一貫)。
- 配線を app 層へ集約: `CommitDraft` の値変化 → 250ms debounce (gen counter、
  既存 `schedule_modal_replan` 機構踏襲) → `cx.background_spawn` で `save_draft`/
  `clear_draft`。空 message は削除。
- ライフサイクル: repo open / branch 切替で `load_draft` (空 input のみ上書き)、
  `execute_commit`/`execute_amend` 成功で `clear_draft`、branch 切替で
  現 branch save → 新 branch load。template draft は展開 plain text を保存し、
  復元時 `parse_message` で構造化 (ADR-0042、現状実装どおり)。

### 3.5 smart-commit (rule + LLM) を async app task として
- domain: `rule_based()` (pure、必ず非空) と Ollama 用 prompt/JSON ヘルパ。
- git-backend: `collect_staged_files`/`collect_staged_diff` (**staged のみ**、
  truncate)。
- app: 2 つの background task —
  1. **detection** (`ensure_smart_commit_detection`): reachability + `/api/tags`
     のみ、diff は送らない、repo ごと 1 回、`KAGI_OFFLINE` で skip。
  2. **generation** (`run_smart_generation`): 「Generate ボタン押下」時のみ起動 →
     `collect_staged_diff` → `generate_message(Ollama)` → 失敗時 `rule_based` に
     静かに fallback → 結果を `CommitDraft` に書き戻す。
- 安全ゲート (ADR-0044、絶対不変): 既定 disabled、初回 `Consent` ダイアログ
  (`CONSENT_LINES` 4 文言 verbatim)、model selection (1つでも初回確認、複数は
  必須選択)、選択 model は settings.json 永続化、`llm_offered = detected &&
  enabled && !offline`。
- **`pending_smart_msg` を廃止**: §3.2 の通り結果を view-model 文字列に書けば、
  次 render の Window で InputState へ流せる (専用 Option フィールド不要)。

### 3.6 IME-aware inputs (gpui-component)
- commit message・template 6 フィールドは `InputState` (IME 対応、採用済)。
  template は `Form`/`Field` レイアウト候補 (audit S 評価)。複数行は
  `InputState` の text/code_editor モード。テーマは `gpui-component` の sync を
  一度呼んで自前パレットで上書き (audit §既知の落とし穴)。

---

## 4. 採用しない設計

- **GitButler コード流用** — FSL ライセンス、concept のみ (ADR-0031)。
- **virtual branch / workspace commit / AI auto-commit** — MVP 思想衝突、Reject。
- **外部 / remote LLM (OpenAI/Anthropic 等)** — ADR-0044: 別 ADR 無しに実装しない。
  対象は localhost Ollama のみ。`but-llm` 型のマルチプロバイダ統合は不採用。
- **`commit.amend(...)`** — ADR-0040: 使わない (new commit + ref 移動で
  cherry-pick/revert と同規則に乗せる)。
- **`reset --hard` / `git clean` / checkout 系を Undo に追加** — ADR-0011/0041:
  絶対禁止 (WT/index 不変で「変更が消える」事故を構造的に排除)。
- **pushed amend (通常モード)** — ADR-0040: blocker (案B)。案C
  (force-with-lease flow) は v0.2+ 別チケット、MVP では実装しない。
- **autosquash 実行 (`rebase -i --autosquash`)** — ADR-0045: 履歴書き換えのため
  MVP 外。`fixup!`/`squash!` prefix commit を**作るだけ**。
- **serde 導入** — drafts/message_gen の手書き JSON 方針を維持。
- **template 構造化フィールドの分解保存** — ADR-0042: draft は展開 plain text を
  保存 (復元の確実性優先)。
- **streaming LLM 表示** — ADR-0044: `stream:false`、MVP 外。
- **commit message editor のリッチ化 (markdown preview 等)** — スコープ外。

---

## 5. リスク (特に LLM safety invariants)

### 5.1 LLM safety (最重要、回帰防止)
- **staged diff のみ送信**: `collect_staged_diff` が HEAD→index 限定であること、
  unstaged/untracked を絶対に含めないことを test で固定。re-arch のリファクタで
  「全 diff」へ退化しないようガード。
- **localhost-only**: 宛先は loopback Ollama のみ。`KAGI_OLLAMA_HOST` override
  はあるが外部 API バックエンドは追加禁止 (別 ADR)。
- **opt-in ゲート**: 既定 disabled、初回 Consent (4 文言)、明示 enable + Generate
  押下が揃った時のみ送信。`llm_offered()` の AND 条件を崩さない。
- **`KAGI_OFFLINE=1`**: detection/generation を完全停止し常に rule-based。headless
  fixture の決定性をこれに依存 (CI が外部に出ない不変条件)。
- **secret 漏洩**: staged diff に secret が含まれ得る (checklist は warn のみ)。
  Consent 文言 "Secrets may still exist…" を verbatim 維持。re-arch で
  「checklist block → LLM へ送らない」連動を検討余地 (現状は独立)。
- **timeout / 失敗の静音 fallback**: HTTP 20s global timeout、失敗は Err →
  rule_based。モーダル/バナーで止めない (ADR-0044)。

### 5.2 commit/amend safety
- **WT/index 不変**: stage/unstage/commit/undo が WT を触らない不変条件を test で
  維持 (現状 doc + test あり)。
- **ref-order 規則 (amend)**: blocker 確認後に最後に ref を動かす。new commit
  作成と ref 移動を分離 (ADR-0040)。
- **pushed amend blocker / 2段階確認**: 通常 amend は blocker、未 push amend は
  2段階 armed 確認 (history-rewriting、ADR-0023/0040)。
- **conflict marker = block (override 不可)**: staged BLOB に marker → 絶対 block
  (ADR-0043 rule 4)。`text_has_conflict_marker` を conflict buffer と共有するため
  両者で挙動が一致する必要。

### 5.3 re-arch 固有リスク
- **UI→git2 直呼びの除去**: `commit_panel.rs`/`mod.rs` の `Repository::open` を
  app 層へ移す際、staging 反映の即時性 (stage→list 更新) を保つ配線が必要。
- **状態集約のリグレッション**: `CommitDraft` への集約で draft debounce 世代管理
  (`draft_save_gen`) や smart-commit の Window タイミング (`pending_smart_msg`)
  を壊さないこと。既存 test (drafts_test / message_gen_test 等) で担保。
- **draft 破損で commit を妨げない**: 寛容 read を維持 (ADR-0042)。
- **手書き JSON / 自前 SHA-1 の脆弱性**: SHA-1 は filename key 用途のみ (security
  非依存)、escaping は oplog と同一実装を共有し乖離させない。

---

## 6. 未解決事項

1. **commit-draft view-model の正確な境界** — `CommitDraft` に amend_modal/
   plan_modal まで含めるか、それらは別 modal stack か。AppState の他 modal
   (sub-agent #? の確認系) との整合が要る。
2. **app 層の repo service 形** — `commit_panel.rs::reload_status` を移す先が
   専用 `RepoService` entity か、AppState 直メソッドか (Zed は store entity)。
   他サブエージェントの backend service 設計と要すり合わせ。
3. **checklist の plan 統合粒度** — rule 1–6 を `evaluate_commit` に一本化するか、
   ADR-0043 の現状二分 (plan 内 1–3 / checklist 4–6) を維持するか。
4. **secret 検出 → LLM 送信ブロックの連動** — 現状独立。checklist warn が出た
   staged diff を Generate でブロック/再確認すべきか (ADR-0044 は Consent 文言で
   注意喚起のみ)。UX 決定が要る。
5. **override の oplog note 配線** — ADR-0039/0043 が要求する
   `overrode: secret/large_binary` note の実装状況が未確認 (本調査範囲では未検出)。
6. **branch 切替時の draft save→load タイミング** — branch 切替フローが他機能
   (checkout) と交差。dirty checkout policy (ADR-0051) との順序関係が未整理。
7. **template draft の structured 復元精度** — `parse_message` は Test:/Risk: を
   body に残す (往復非対称)。v0.2 で構造化保存に拡張するか (ADR-0042 は MVP 外)。
8. **smart-commit の body/test/risk 生成** — ADR-0044 で v0.2 扱い。template
   モードとの統合 (LLM が 6 フィールドを埋める) は未設計。
