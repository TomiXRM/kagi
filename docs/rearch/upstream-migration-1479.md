# #1479 / #436 — fork 解消の確定実装計画（owner 決定反映）

## 0. 決定・基準点・この文書の意味

本書のslice識別子は必ず `#1479-S0`〜`#1479-S6`（分割は `#1479-S5A`〜`#1479-S5D`）とする。`docs/rearch/migration/README.md` のstrangler plan S1–S7とは別計画であり、番号に対応関係はない。U1は本issueのupstream prerequisiteを指す。slice一覧・依存・受入条件は本書を正本とする。

- **採用する**: official gpui-kit main が選んだ crates.io `gpui-pre =0.3.9` と同系列 platform/web/wgpu。Kagi の最終状態は **全 manifest の `[patch]` がゼロ**。
- **廃止する保証**: Kagi独自の、Issue/PRの画面外を含む複数投稿選択・Copy・会話全体Select All。**投稿単位の選択・Copy・Select All は公式の挙動のまま残す**。跨ぎ選択を保証・維持しないだけで、公式がmounted範囲で自然に許す跨ぎ動作は**禁止もしない**。公式 TextView/default scopeを改変せず使い、ADR-0198を改訂する。旧報告 #1479-S3 の独自 rich-selection renderer 案は取り下げる。
- 調査基準: **origin/dev `c0a4ea3ab6b3dc6aa68c2fdb2c379a4eefd53f64`**。本書の Kagi `file:line` はこのコミットに対応する歴史的な調査位置であり、現在の行番号ではない。実装開始時に差分を照合する。
- 最終依存先の検証済み候補: official `longbridge/gpui-component@1188c11003b5cb5b83bc07d3fb9112e7674fbfa7`。main を無固定で追わず、component/kit-assets を同じ rev に固定する。gpui-pre は Zed 自身の crates.io リリースではなく、official Kit が採用する snapshot package である、という出自も明記する。
- 出典: [#1479 の調査・owner決定コメント](https://github.com/TomiXRM/kagi/issues/1479)。fork差分・upstream比較・依存解決・過去計測は同コメントの調査報告を参照する。
- **本追補ではビルド・テストを実行していない**。以下の gate は実装担当/main が各 merge 前に行う受入条件であって PASS 報告ではない。`[INFERENCE]` の LOC 見積もりや proposed 閾値を実測値と混同しない。

### 証拠リンクの読み方

Kagi: `https://github.com/TomiXRM/kagi/blob/c0a4ea3ab6b3dc6aa68c2fdb2c379a4eefd53f64/<path>#L<line>`。
Kit: `https://github.com/longbridge/gpui-component/blob/1188c11003b5cb5b83bc07d3fb9112e7674fbfa7/<path>#L<line>`。
ローカルの全 `file:line` は上記 SHA に対応する。調査後 dev が進んだら、実装開始時に line mapping と衝突箇所のみ再確認し、既存の新しい変更を上書きしない。

- Upstream API candidate (#1606): let `SearchPanel` forward a host-supplied context-menu builder or locale to its internal Find/Replace `Input`s; until a public API exists, those fields retain gpui-kit’s default English menu while Kagi-rendered fields use the shared localized builder (no fork/pin changes).

- Current transitional patch (#1816, 2026-10-27): the complete Zed family shares
  [`edfc6891ab91b3ef783b25114b355fd576063a37`](https://github.com/TomiXRM/zed/commit/edfc6891ab91b3ef783b25114b355fd576063a37).
  This descends from `caf5007` without changing its list estimate/scroll fixes,
  and adds Unicode line-break/grapheme candidates to shaped-label wrapping.
  The emergency fallback protects punctuation and grapheme boundaries while
  still breaking overlong Latin tokens; the editor `LineWrapper` is unchanged.
  This is not a claim of all-Unicode conformance. No app-side prebroken strings
  or joiner insertion substitutes for the shared renderer fix. Removal in
  #1479-S6 requires preserving the existing list geometry, shaped-text
  punctuation/grapheme and text measurement cache regressions on the official
  family. The measurement cache keys both wrap and truncation constraints:
  an unconstrained intrinsic-size query must not reuse a previously truncated
  ellipsis. Before paint, the original node measurement closure reconciles
  shaping with final bounds if layout selected geometry from an earlier measure.
  The app separately constrains each theme caption's actual text leaf to its
  allocated width, preserving full accessible names and hover help.
  The `caf5007` source links below remain historical investigation evidence.

### 全 slice 共通の merge 規則

1. regression は **同じ PR 内で先に書く**（必要なら手元で red→green）。失敗するテストだけを dev に merge しない。挙動変更・テスト oracle・ADR/changelog を一つの green merge unit にする。
2. 準備 slice は現行 pin でビルド可能な public API だけを使う。新しい base Radio / typed Editor / search engine は最終 #1479-S6 まで merge しない。#1479-S6 は manifest 変更だけ先に入れず、型移行と全 consumer/test を同時に切り替える。
3. 一度完了した切替に古い API alias・feature flag による旧新両系統・fork 復帰 fallback は残さない。途中の pin は次 slice の prerequisite にすぎない。
4. merge 前の共通 gate: workspace unit/integration、変更対象 native GUI filter、default と gui-e2e の compile。#1479-S6 はこれに Linux/Windows/WASM の実 target compile と web harness を加える。renderer metadata を OS AX/IME/pixel の実測と呼ばない。
5. native runner は `.claude/skills/verify/SKILL.md:49,515–532,590–592` に従う。`KAGI_GUI_E2E_ONLY` は **カンマ区切り部分一致**、KEEP_GOING は明示 filter 必須。新 scenario は下記で「新規」と記した名前を登録する。存在しない filter で 0 件 PASS にしない。

```sh
# 実装後に main が行う例。調査中には実行していない。
KAGI_LOG_DIR="$(mktemp -d)" KAGI_GUI_E2E=1 \
  KAGI_GUI_E2E_KEEP_GOING=1 KAGI_GUI_E2E_ONLY='<slice の filter>' \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo test -p kagi --features gui-e2e --test gui_e2e_runner -- --nocapture
KAGI_LOG_DIR="$(mktemp -d)" cargo test -j 8 --workspace
```

**件数/LOC の定義**: files_count は下記で列挙する予定変更パス（新規/削除含む）の distinct 数。受入のみで変更しないファイルは数えない。slice 間の重複はあるので合計は unique file 数ではない。LOC は追加＋削除の churn（Rust/test/docs/TOML を含む、生成 Cargo.lock の行数は除外）の `[INFERENCE]` 幅。未ビルドのため最終 #1479-S6 の compiler-driven minor fixes は予備幅で明示する。


## 1. 追加調査とownerによる仕様解釈の確定

### U0は不要 — mounted cross-viewを禁止するscopeは作らない

公式のTextViewはwindow-level selection controllerを共有し、mounted複数viewのdrag/Shift-clickを自然に扱う（official `component/src/text/window_selection.rs:1703,1858`）。独立scopeを作るとComponent Rootのdefault/modal scope管理と競合するが、**ownerはその独立scope/境界保証を求めていない**。

したがって **per-view selection-boundary APIのupstream PRは作らない、待たない**。Kagiのgroup/lease/全post canonical interval/outer edge autoscrollを除去し、公式TextView/default scopeをそのまま採用する。公式に残るmounted cross-viewを失敗とする負条件テストも追加しない。「単一投稿の正のCopy証明」と「Kagi独自の画面外全会話保証を終了すること」を分ける。

旧報告の独自rich-selection renderer案、追加調査途中のstrict per-post scope案はいずれも撤回する。依存graph上のzero-patchは確認済みであり、selectionに関する外部prerequisiteはない。

### composerの公開API構成は最後に別途確定

現 `commit_panel_composer_render.rs:28–35,79–91` のpreferred3–12行と低windowでのshrink/internal-scrollは引き続き維持する。Textareaのauto_growはleaf min-heightも増やす (`base/input/base/element.rs:2518–2524`) ので、単にauto_grow+外側min_h0では同じでない。**Kagi側の高さ設定＋plain Textareaの内部scrollという公開API構成を先に検討し、必要がある場合だけ公式cached-row getterをprerequisiteにする**。最終判断と具体的構成は#1479-S6に記す。不要なEditorState偽装・別shaper・forkを先に選ばない。

- Upstream candidate (#1607, unpublished): expose public `SearchNext` / `SearchPrevious` actions (and selection-to-query support) in official gpui-kit. The pinned `7a116a2` exports only `input::Search`; its `SearchPanel`, navigation methods and owning field are private. Until public actions exist, Kagi offers working Find… only, not disabled placeholders for Next/Previous/Use Selection, and does not change dependency pins.

<a id="native-termination-u2"></a>

### U2 candidate — macOS native application termination admission（#1614）

2026-10-15 の **read-only source audit**。build / test / native OS termination は実行していない。現在の Kagi pin と crates.io の公開最新版（docs.rs `latest` の表示は `gpui 0.2.2` / `gpui-pre 0.3.9`）には、アプリ利用者が macOS `applicationShouldTerminate:` を拒否・非同期保留する公開 API は見つからない。window の `on_should_close` は application termination の契約ではない。

| 調査対象 / version | 一次資料と判定 |
|---|---|
| 現行 lock: `gpui 0.2.2` / `TomiXRM/zed@caf5007618c309a8ea66002d44a9c9ab629a2d35` | [`crates/gpui/src/app.rs:2177–2184`](https://github.com/TomiXRM/zed/blob/caf5007618c309a8ea66002d44a9c9ab629a2d35/crates/gpui/src/app.rs#L2177-L2184) は `on_app_quit` で cancel 不可を明記。`SHUTDOWN_TIMEOUT` は 200ms。[`crates/gpui_macos/src/platform.rs`](https://github.com/TomiXRM/zed/blob/caf5007618c309a8ea66002d44a9c9ab629a2d35/crates/gpui_macos/src/platform.rs#L95-L98) は `applicationWillTerminate:` を登録し、`will_terminate` が `on_quit` を通知する。should-terminate / deferred reply hook はない。 |
| Zed 公式 crates.io `gpui 0.2.2` | [`src/app.rs:1769–1777`](https://docs.rs/crate/gpui/0.2.2/source/src/app.rs#1771) の同 API も cancel 不可、cleanup timeout は 100ms。[`src/platform/mac/platform.rs`](https://docs.rs/crate/gpui/0.2.2/source/src/platform/mac/platform.rs) の delegate は `applicationWillTerminate:` / `will_terminate`、`on_quit: FnMut()`。`applicationShouldTerminate:` / later reply はない。 |
| official Kit 採用 snapshot: crates.io `gpui-pre 0.3.9` + `gpui-pre-macos 0.3.9`（Zed 自身の公開 package ではない） | [`src/app.rs:2714–2721`](https://docs.rs/crate/gpui-pre/0.3.9/source/src/app.rs#2716) も cancel 不可、native timeout は 200ms。[`src/platform.rs:457`](https://docs.rs/crate/gpui-pre/0.3.9/source/src/platform.rs#457) の `on_quit: FnMut() -> bool` は利用者向け veto ではない。`App::new_app` 内の callback は AppCell borrow 成否を返すだけ（`src/app.rs:1087–1105`）。[`gpui-pre-macos/src/platform.rs:103–106,1381–1388`](https://docs.rs/crate/gpui-pre-macos/0.3.9/source/src/platform.rs#1381) は引き続き `applicationWillTerminate:` で通知し bool を使わない。 |

| Upstream candidate | 要件 / owner / removal condition |
|---|---|
| **U2: application termination veto/deferral（#1614 の唯一の残項目）** | **Gap:** Dock Quit / logout / shutdown の通常の AppKit termination request を Kagi の共有 host-close admission / Save All・Discard・Cancel に接続できない。終了後 cleanup future を確認ダイアログの代替にせず、window-close veto で防げるとも主張しない。**Owner:** repository owner が下記 English proposal を Zed GPUI upstream の PR/Discussion として公開し、その URL と採用 revision/API をここ・`docs/decisions.md` に記録する（現時点は未公開、リンク未取得）。**Removal:** official public API が native request を終了前に veto/defer でき、deferred reply を返せる版へ明示移行し、Kagi の既存共有 guard に接続して Dock Quit / logout / shutdown の Cancel・Save failure・busy hold・successful exit を隔離 native 環境で受け入れたときに gap と #1614 を閉じる。fork / dependency pin 変更 / Objective-C delegate 差替え shim は追加しない。U2 は #1479-S6 の既存 zero-patch cutover の新 prerequisite にはしない。 |

**Unpublished owner proposal (English):** “Expose application termination admission before shutdown. On macOS, route `applicationShouldTerminate:` through a public application callback that can allow, cancel, or defer termination, and expose completion of a deferred request through `replyToApplicationShouldTerminate:`. Keep this distinct from non-cancellable `on_app_quit` cleanup. Kagi needs to run its existing aggregate unsaved-buffer guard, respect active write/clone admission, and cancel when saving fails or the user chooses Cancel, including Dock Quit and normal logout/shutdown requests. Please document repeated-request/reply lifetime and platform behavior; forced process termination is outside this guarantee.”

Kagi が所有する menu/shortcut/Settings Quit・host/window close の共有 guard 修正とは分離する。**その実装の受入後も #1614 は U2 のみで open のまま**とし、上記 source audit を native runtime の再現・受入成功とは呼ばない。


## #1479-S0 — 方針と出自を固定する（最初に merge）

**依存**: なし。**merge unit**: 文書・現行コメントのみ。依存更新・API変更なし。

**変更パス（6）**:
1. `AGENTS.md:13–37`: upstream-as-is / zero-patch / owner 承認された範囲変更 / Kagi-owned boundary の不変条件。
2. `docs/adr/0001-gpui.md:21–30`: 0.2.2 固定・platform 未公開という歴史記述を保持し、新決定 gpui-pre を追記。
3. `docs/adr/0006-gpui-component.md:19–30,43–44`: 0.5.x 留保を supersede、official main 同一 rev / typed API 移行を採用。
4. `Cargo.toml:177–179`:「cosmetic polish が欲しければ fork を re-add」の指示を削除。今ある patch をここで早まって削除しない。
5. `docs/decisions.md:25,143–146,279,300–302`: 過去決定を書き換えて証拠を消さず、先頭に新しい superseding decision を一件追加。
6. `CHANGELOG.md:6`: migration 方針の変更を記録。

`CLAUDE.md` は git mode **120000**、内容 `AGENTS.md` の symlink。別内容を複製・上書きしない。物理変更ファイル数は AGENTS 一つで数える。

**不変条件の文案**:
> ライブラリは公式 upstream の公開 API を原則そのまま使用する。移行中は #1479 に列挙された既存 pin の削減だけを許し、新しい fork/patch を増やさない。#1479-S6 完了後の Kagi dependency graph は `[patch]` を持たず、gpui 系は official gpui-kit が採用する gpui-pre の同一 package identity に統一する。足りない要件はまず Kagi 所有の domain/UI 境界で解決し、必要なら upstream 提案を行う。依存ライブラリの fork・ソースコピー・隠し互換 shim を便宜的に再導入しない。仕様縮小は owner の明示決定・ADR・consumer oracle の同時更新を要する。Kagi-owned terminal は出自と MIT/Apache notices を保持する例外的な所有コードであり、他ライブラリの fork を正当化しない。

**regression/gate**: docs のリンク・CLAUDE symlink維持、Cargo の実依存内容が不変。共通 gate の既存 baselineを利用し、新しい native scenario は不要。**LOC**: 90–160。


## #1479-S1 — 明示 fallback を全 native shaping 経路に通して cosmic patch を除去

**依存**: #1479-S0、#1479-S2（terminal 所有方針を先に確定）。**merge unit**: fallback plumbing＋テスト＋cosmic patch/lock除去を一緒に merge。component / Zed pin はまだ現状維持。

### production の変更位置

| Kagi file:line | 変更 |
|---|---|
| `vendor/gpui-terminal/src/view.rs:141,170,505,1208–1220` | `TerminalConfig` に `font_fallbacks: Option<gpui::FontFallbacks>`（既定 None）、renderer作成・live config変更へ渡す。Kagi caller が明示するので汎用既定を勝手に Noto にしない。 |
| `vendor/gpui-terminal/src/render.rs:170,213,247,633` | renderer に fallback を保持。ASCII M の metrics と実 glyph 描画の **両方の Font** に設定。Arc-backed 値の clone を使い、各 glyph ごとの Vec 構築なし。 |
| `src/ui/terminal.rs:111–132` | 唯一の production `build_terminal_config` が `kagi_ui_core::fonts::ui_font().fallbacks` を入れる。`539–551,563–620` の初期font選択と既存session更新経路をそのまま通す。 |
| `src/ui/commands.rs:2082–2114` | 既存 all-session live update の **受入確認箇所**。新規の第二更新ルートは作らない（基本変更不要）。 |
| `src/ui/render_status.rs:87–103`（caller `181,191,335`） | Fresh `Font` に Root の fallback が継承されるという誤解を除去し `ui_font()` 由来の shaping と描画を一致させる。 |
| `vendor/gpui-terminal/src/view.rs:113` / `src/lib.rs:60,147` / `src/main.rs:130` / `README.md:48` | 全 struct literal/doc/example を新 field に追随。renderのconstructor例 `render.rs:54,211,725,768` も同時変更。 |
| `Cargo.toml:171–176` / `Cargo.lock` | cosmic fork patchのみ除去、通常 registry 0.19 系へ解決。不要になった空 `[patch.crates-io]` も残さない。 |
| `docs/adr/0130-bundled-japanese-font-fallback.md:29–31` | 「implicit pathを保護するため patchを残す」を明示fallback完了により supersede。static Regular/Bold と OFL は維持。 |

### regression を先に置く

- `crates/kagi-ui-core/src/fonts.rs:28,40,58,87–137` の既存実 Cos­micTextSystem テストを拡張。`ABC日本語直骨誤`、Normal/Bold、UI Inter / terminal JetBrains Mono / 実在する Nerd 系 primary、ASCII と2-cell CJKを対象にする。**run が2個あるだけでは不十分**。同じ text system で primary を明示 `Noto Sans JP` にした参照 shape の日本語 run FontId と一致、ASCII run は primary、Regular/Bold は対応する別 static face と照合する。非公開 `TextSystem::font_id` を呼ぶ設計にはしない。
- `vendor/gpui-terminal/src/render.rs:724–779` 付近の renderer unit と `view.rs` config update unit: clone/new/live update の fallback伝搬、font変更でmetrics再計算、null/defaultの汎用契約を確認。
- **新規** `tests/recovery/terminal_fonts.rs`: `terminal_font_fallback` 登録。実terminalのfont切替・zoom・セッション切替・再起動で同じconfigが伝わること、ANSI bold / CJK2セル / ASCIIカーソル位置を確認。PTY に日本語を送れたことだけを glyph保証としない。
- 計測側も production と同じfallbackにする: `tests/recovery/status_bar_fit.rs:202`, `time_locale.rs:112`, `density_rest.rs:97–120`, `finder_scale_editor.rs:88–95`, `finder_scale_history.rs:78–85`, `finder_scale_rest.rs:137–141`, `home_heading.rs:65–73`, `inspector_scale.rs:91–98`。Window.text_style 外で Fontを作るprobeは Rootを継承しない。

### Linux の必須受入（macOS PASS で代替不可）

1. Linux runner上で `cargo test -p kagi-ui-core linux_cosmic_text_uses_bundled_fallback_for_regular_and_bold` と新しい同系font単体テスト。既存 `new_without_system_fonts` を使い、bundled facesのみで ASCII/日本語が正しい face にshapeされることを検証。
2. 別の隔離 Linux native run（X11、Waylandはそれぞれ対応環境で）。`ja_JP.UTF-8` と `en_US.UTF-8` の新規プロセス、system Noto SC/JP がある環境と最小font環境の両方。個人PCのfont削除/グローバルfontconfig変更は禁止。用意したVM/containerのprivate fontconfig/cacheで行う。
3. terminal に `ABC 日本語 直骨誤` を通常/ANSI boldで出し、同じwindowに明示 Noto Sans JP の参照描画を並べたテストfixtureまたは隔離 captureで比較する。Regular/Bold・glyph形・2-cell位置・selected font と fallback の役割を残す。native画像はpid指定captureを使う。Inter根の一般ラベル、status fitting、commit/Issue Input、TextViewでも日本語が同じfallbackになることを確認。
4. 既存 `tests/gui_e2e_runner.rs` は macOS gated moduleが多い（例 `48,872–891`）。このrunnerをLinuxで空実行してPASSとせず、Linux実shape unit＋native実機/Tier Bを独立証拠にする。#1479-S6後の pre移行でも同じLinux gateを再実行。
5. 未収録の全Unicodeや、primary font自体が持つSC glyphの強制置換は保証しない。明示fallbackはprimaryにglyphがない場合に効く。Nerdのicon優先順位を壊さない。

**native filters（既存＋新規）**: `terminal_,status_bar_,relative_time_ja,cleanup_local_date,oplog_row_typography,issues_row_secondary_text,finder_scale_rest,home_heading,inspector_scale`。`finder_scale_rest` が editor/history のinspectを内部実行する（同file:113–114）。`terminal_font_fallback` は新規登録を必須にする。terminal paste / control / focus / auto-lock の既存oraclesは変更しない。

**変更パス24**: 上表のroot2、vendor5、production2、fonts1、計測test8、新nativetest1、runner1、ADR-0130/decisions/CHANGELOG/verify skill4。**LOC**: 550–950（lock再解決を除く）。**リスク**: font fallback fieldを初期化時だけ渡してlive設定を落とすこと、実測probeと描画fontの乖離、Linuxで偽の空suite。


## #1479-S2 — terminal を Kagi-owned と明文化（#1479-S1 より先に merge 可）

**依存**: #1479-S0。**merge unit**: 所有/出自/noticeの説明のみ。terminal実装・公開挙動は変えない。

**変更パス（6）**:
- `docs/adr/0035-vendor-gpui-terminal.md:1,14–30`:「temporary fork」「upstreamにselectionが来たら即削除」を撤回。terminal frontend/selection/PTy統合はKagi所有、VTEは引き続きofficial `alacritty_terminal`、upstream出自は追跡する。
- `vendor/gpui-terminal/README.md:1–3,60–67`: upstream一般説明とKagi変更を区別し、既に実装済みのselection/scrollbackを「未実装」とする古いlimitationを除く。
- `vendor/gpui-terminal/Cargo.toml:1–10,23–26`: normalized registry copyのまま「自動生成で編集しない」という誤説明を修正し、Kagi-owned packageの説明とupstream originを両方明示。名前の変更・directory移動はしない。元authorの出自は消さない。
- `Cargo.toml:51`: vendored forkという一時性コメントをKagi-owned/ADR参照に更新。
- `docs/decisions.md`, `CHANGELOG.md`。

**license**: `LICENSE-MIT` / `LICENSE-APACHE` と attribution をそのまま保持。MIT OR Apache-2.0のupstream由来をKagi著作だけに置き換えない。Zed terminal（GPL）への転写・丸ごと置換は採らない。機械的 rename とemulator rewriteは本計画のscope外。

**regression/gate**: code diffがないこと、packagingにnoticeが残ること。既存 `terminal_paste_bracketed,terminal_paste_control,terminal_focus_exit,terminal_hide_focus,terminal_states,terminal_auto_lock` を隣接baselineとする。**LOC**: 110–190。**リスク**:「owned」を理由にupstream noticesを消すこと、単なるdirectory renameをfork解消と誤認すること。


## #1479-S3 — 投稿単位の正条件を固める準備（旧 #1479-S3 を置換）

**依存**: #1479-S0。**merge unit**: 現forkでも満たせる単一投稿の正条件、mounted/unmounted witnessのみ。実際のgroup除去・公式挙動へのoracle切替・ADR現仕様改訂は **#1479-S6の原子的cutoverに統合**する。これにより旧pinの非managed default selectionと新official controllerを二度実装・検証する寄り道を避ける。

**変更パス4**: `tests/recovery/issue_conversation_selection.rs`, `tests/recovery/issue_offscreen_select_all.rs`, `tests/recovery/issue_conversation_support.rs`, `CHANGELOG.md`。

- `selection.rs:67–114`: 巨大単一postのexact Unicode/rendered-text Copyを明確にする。
- `selection.rs:116–195`: reorder/prepend後に選択postが本当にmountedであるwitnessを追加。outer-unmountでもretainする旧要求と混同しない。
- `offscreen_select_all.rs:76–162`: unmount前のmounted partial→Cmd-A/fullの正条件を追加。旧offscreen oracleの改訂は#1479-S6まで待つ。
- support `:168–177,223–238`: 新frameのpaint bounds witnessを使う既存physical drag/poison clipboard helperを再利用。graph_focusもimportするのでmoduleごと消さない。

**gate filters**: `issue_conversation_giant_copy,issue_conversation_accepted_identity,issue_conversation_offscreen_select_all,graph_copy_foreground`。**LOC**: 250–400。準備テストだけで仕様変更完了と呼ばない。

### #1479-S6で切り替える既存13 scenarioの全oracle

登録 `tests/gui_e2e_runner.rs:4089–4138`。公式の偶然のmounted cross-viewを禁止するassertは設けない。新方針で不要な旧保証を、明示的に意味を変更した単一投稿testへ置換する。

| 既存scenario / 登録line | owner決定後のoracle |
|---|---|
| `issue_conversation_offscreen_copy` /4089 (`selection.rs:27–65`) | A→Z/逆順・未paint Mを含むALPHABET全文保証を**終了**。A内のpartial/fullを確立→Copy exact A、別のZをfresh選択→exact Zという単一投稿正条件へ。A選択後outer rowを実unmountしたframeでは旧Aのselection authorityを保持しない。複数mounted post間dragの結果をKagiの固定期待値にしない。 |
| `issue_conversation_giant_copy` /4093 (`:67–114`) | **保持**: 一つのpostのGIANT_000〜179 exact全文、same-update move→Copy、末尾/Reply到達。`.scrollable(false)`なので内部block virtualizationの証明ではない。 |
| `issue_conversation_accepted_identity` /4097 (`:116–195`) | mounted Bは同author/time reorder/prependでもOLD_B保持。accepted OLD_B→NEW_B/deletionはbefore-paint POISON、fresh選択でNEW_B。reorderで実unmountした場合は公式clearに従う。 |
| `issue_conversation_refresh_anchor` /4101 (`:197–255`) | **保持**: unchanged refresh/前方insert-deleteのreading PostKey+intra-y、失敗時accepted mounted M Copy/anchor/error chrome。 |
| `issue_conversation_geometry` /4105 (`:256–286`) | **保持**: exact Unicode/U+2009 code literal、sidebar幅/zoom/theme変更後mounted partial Copyとfresh drag。scopeを独自activateしない。 |
| `issue_conversation_edge_drag` /4109 (`:288–372`) | **outer conversation edge-scroll保証を終了**。旧positive全会話traversalを除去し、single-post gesture終了/departure後にKagiの残留outer taskがscrollしない回帰へ置換。公式がmounted間でどこまで選ぶかを禁止しない。内部scrollのpositiveは別single-post test。 |
| `issue_conversation_delayed_rejected` /4113 (`lifecycle.rs:124`) | **保持**: refresh中accepted A、superseded completion拒否、repo retarget→同update CopyがPOISON、late A復活なし。 |
| `issue_conversation_return_pending` /4117 (`lifecycle.rs:50`) | **保持**: 旧pending readが再開generationへsettleしない、Loading/Reply→新accepted、fresh選択。 |
| `issue_conversation_return_failed` /4121 (`lifecycle.rs:89`) | **保持**: unaccepted失敗→departure→returnのLoading/Error/Reply、ghost body無し。 |
| `issue_conversation_two_owners` /4125 (`lifecycle.rs:199`) | **保持**: 実session/repo B、tabswitch/ownerclose before-paint POISON、background accepted AがBで権限を持たない。同issue番号でもowner別。 |
| `issue_conversation_copy_priority` /4129 (`lifecycle.rs:314`) | **保持**: Reply Input全選択/Copy優先、modal表示でbase即withdraw、modal閉じても復活なし、mode/Home離脱即POISON。 |
| `issue_conversation_enabled_input_copy` /4133 (`issue_enabled_input_copy.rs:75–164`) | setupのbody+comment全文をbody**だけ**のexact Unicodeに変更。Input draft全選択/空Input Copy consumes/POISONは保持。programmatic Input focus→physical Tabで元**mounted post**の選択へofficial fallback。クリックInputによる公式selection-clearとは別witnessにする。 |
| `issue_conversation_offscreen_select_all` /4137 (`issue_offscreen_select_all.rs:76–162`) | outer-unmount後PARTIAL維持→Cmd-Aでpost全文の**Kagi独自fallbackを撤去**。実unmount後Copy/Cmd-A/Cは旧postを復活させない。単に戻しただけでは旧selection復活なし。fresh physical focus→Cmd-A/Cは元のexact full rendered GFM/Unicode。draft/producer/repo不変条件は維持。 |

画面外単一投稿の全文取得は選択controllerで復元しない。後述の調査のとおり、既存の投稿本文Copyボタンも存在しない。目的の投稿を再表示して公式操作する方針であり、会話全体Copyの代替機能は新設しない。

### #1479-S6で追加する実native scenario（新規登録）

`tests/recovery/conversation_official_selection.rs`（新規）:
- `issue_conversation_post_select_all`: mounted body/comment各単独のpartial/full、rendered Unicode/GFM/コードとprivacy、公式Copy/SelectAll。
- `pr_conversation_post_select_all`: description/review/commentの単独Copy、同number別repo/session、accepted source差替え。既存 `pr_same_number`/`pr_threads` はclipboardを検証していないので代用不可。
- `pr_thread_post_select_all`: inline thread comment単独のpositive。既存full-hunk Copy Buttonは別契約のまま。
- `issue_conversation_official_rerender`: mounted partialを保った実Root再render・chrome更新・theme/width/zoom。default scope維持を確認。
- `issue_conversation_giant_internal_selection`: bounded parent + official `.scrollable(true)` の**テスト用実wrapper**で同TextViewがmountedのままinternal blockを仮想化し、single-post全文Copy/内部edge task停止。productionを勝手にnested-scroll化しない。

**unmounted focus**: 公式のactual-unmount cleanup (`base/text_selection.rs:1294–1323`, `base/text/selection_adapter.rs:147–158`) を採用。retained Entityを理由に旧postへSelectAllを転送しない。Kagi既存のfocus回復経路を使い、隣post/Replyへ自動SelectAllしない。Input/modalが既にfocusを持つ場合は触らない。独自scope/presence registry/全post走査を新設しない。

**source replacement/departure**: official set_textはprefix追加をstream appendとしてselectionを保持する (`base/text/state.rs:315–335`)。accepted snapshot差替えはstreamではないので、selected旧sourceを明示clearしてから入れ替える。source/deletionはcxを持つaccepted mutationで同期clearする。session/modal/Home離脱は既存Kagiのactive/generation guardをCopy/SelectAllのcaptureで検証する（詳細#1479-S6）。任意のno-App DropがSDK内部stateまで即時clearするという旧lease固有保証は維持しない。


## #1479-S4 — 旧 pin 上で Clear / IME / Input AX oracle の依存を外す

**依存**: #1479-S0。#1479-S3と共有するrootファイルはrebaseで統合。**merge unit**: 旧pinで動くpublic Input suffix・GPUI action境界・実native AX inspectionのみ。Find engine / Radio / typed statesはここに混ぜない。

### 対象と実装契約

- `src/ui/sidebar_panes.rs:500–519` の唯一の `clear_button_aria_label` を削除。`Input::new(...).aria_label(...).cleanable(false)` とpublic `suffix`に既存Kagiのnamed Buttonを置く。空ならsuffix無し。押下は同じInputの `clean(window,cx)` と focus復帰。表示size/hitbox/tab順・locale・query ownershipはそのまま。別TextInputや名前付けだけのGroupを増やさない。
- `src/ui/sidebar_focus.rs:195–218` の「非empty Escapeはclear+Input維持、empty Escapeはlistへ」を受入契約として維持（原則変更不要）。IME中のEscapeがそれを先に実行しないようにする。
- 共有公開API helperを**新規** `crates/kagi-ui-core/src/input_actions.rs`、exportを `src/lib.rs:21` 付近に置く。`WindowExt::focused_input` と `EntityInputHandler::marked_text_range` による **action開始時** のIME状態観測を行う。
- Kagi root (`src/ui/render.rs:690–703`, state `src/ui/mod.rs`) と独立Settings root (`src/ui/settings_window.rs:316–372`) の既存containerに `input::Escape` のcapture/bubble境界を設置。**二段式**: captureで現在のfocused Inputのmarked有無をwindow-ownerのboolに上書きして `cx.propagate()`、kit childが通常のmenu/multicursor/inline completion/touch selection/IME順で処理、bubbleまで来て開始時markedならそこでconsume、そうでなければpropagate。次のcaptureで必ずresetする。renderごとのRc/Vec allocationやglobal別selection registryを作らない。
- これにより upstream `base/src/input/base/state.rs:2176–2213` のmenu→cursor→completion→touch→IMEという優先順位を飛ばさず、IME unmark後に別のEscape binding（sidebar clear / modal close）へ落ちることを止める。単純なroot早期unmarkはその優先順位を壊すので採らない。
- 現行Kagiには `.clean_on_escape()` callerがないことをgrep確認済み。将来同設定を導入する場合、childがclearする前に止める必要が生じるのでこの契約も同時改訂する。本sliceは勝手にclean_on_escapeを有効化しない。
- #1479-S6ではhelperのfocused stateをpublic `AnyInputState::{Input,Textarea,Editor}` matchへ変えるだけ。型消去のために全入力を旧InputStateへ戻すshimは作らない。

### tests first

1. `tests/recovery/sidebar_search.rs:119–146` / `sidebar_filter.rs:33,124` / `input_names.rs:10,30–38,66,100,112` の `take_rendered_inputs_for_test` / `rendered_aria_label` を **実AX tree** oracleへ移す。`tests/recovery/native_ax.rs:21–38` の既存 `Window::debug_a11y_tree_json` を再利用。`tree["gpui_focus"]` が指す実 `nodes[id]["aria"]["role"/"label"]` を見る既存先例は `blame_toggle.rs:23–26,51–55`。外側Group名や宣言定数をassertして終わらせない。
2. Clear: emptyで実Button不存在、EN/JA別名、AppKit Pressとpointer双方でqueryのみ消える、TextInput名は不変、focus保持。IME composition→1回Escapeで未確定状態だけ終了→次Escapeでquery clear→さらにEscapeでlist exit。
3. **新規 scenario** `input_ime_escape_boundaries` を既存 `tests/recovery/input_names.rs` 内に追加しrunner登録。Sidebar・Commit body・Issue editor・独立Settings textareaで、IME中のEscapeが祖先のclose/clearに届かず、通常Escapeの既存順序が生きることを検証。menu/completionが開いている場合はその優先消費を保持。button focus時はhidden editorにediting actionを送らない。
4. `editor_find` の fork `SearchPanelProbe` は **まだ削除しない**。新しいsearch engineがない旧pinで代用品を先行投入すると二重実装になる。#1479-S6で実hostとoracleを同時置換する。

**変更パス15**: core新helper/lib、root mod/render/settings_window/sidebar_panes、input_names/sidebar_filter/sidebar_search/native_ax、runner、ADR-0014/decisions/CHANGELOG/verify skill。

**gate filters**: `sidebar_search_,sidebar_filter_tab_entry,input_names_inventory,input_ime_escape_boundaries,settings_,commit_panel_body_focus,compact_rail_cmd_f`（新規名を登録したことをrunner summaryで確認）。**LOC**: 450–750。**リスク**: captureだけでkitのpopup優先を潰すこと、前eventのmarked flag再利用、native TextInputではなく名前付きwrapperを数えること。


## #1479-S5 — zed list fork を不要にする（4つのgreen merge unit）

現snapshotの実検索: `with_uniform_item_height` **7 callsites / 7 files**、`reset_with_uniform_height` **1**、`measure_all` **0**。7 listsという意味ではない。shared diffは**6 retained owners**、restore helperは**2 panes**を支える。

### geometry所有を確定

| 現callsite | 採用するofficial/public-API側の方式 |
|---|---|
| `core/diff_list.rs:71,78` → Main/History/PR/Conflict/Editor WIP/Editor History | Kagi-owned **一つの共有 variable-list Element/handle**。code wrap/split cell/overlayは固定行ではない。既存row renderer/selection/approvalを再実装しない。 |
| `src/ui/pr_lane.rs:237` | official `uniform_list`。elision/commitとも `.h(scaled ROW_H)` (`373,465`)。 |
| `src/ui/inspector_files.rs:140` | official `uniform_list`。dir/More/fold/fileは同slot (`284,299,335,396`)。Unavailable (`307–312`)にも明示同height slotを与える。 |
| `src/ui/operations/oplog_restore_inspection.rs:82` | refs/AFTER graph双方official `uniform_list`。refs `oplog_restore_card.rs:122–123,393–420`、preview `oplog_restore_preview.rs:270–370`。 |
| `src/ui/issue_conversation.rs:104–105` | accepted postsは同じKagi variable owner、既存PostKey/acceptance/generationを再利用。 |
| `src/ui/issues_mode/feed.rs:186` | 同variable owner。`:46`はmin_hにすぎずtitleは`:146–153`でwrapするのでuniform不可。 |
| `src/ui/tab_view.rs:596–601` pending thread | **同variable ownerに統一**。2 outer rowsでもcomposer previewは無制限rich text (`issues_composer.rs:412–424`)。小list用の第二geometry strategyを作らない。 |

### measure_all の採用閾値と実測を混同しない

**本実装のproduction採用閾値は0（使用しない）と確定**。理由は「全件計測で解決できないからゼロ」ではなく、実測で1k coldがすでに1frameを越え、唯一の小候補pendingも巨大previewを許すため、既に必要な共有variable ownerを使う方が単純だから。count-only threshold / 大list scroll_to_endだけ、という案は採らない。

実験上の安全上限を論じるなら **≤256 rows かつ total64KiB / row8KiB / total1024 projected wraps / row64 wraps、画像・table等cost不明なら不採用**が保守的な評価候補。しかし**256の測定は未実施**であり、この条件分岐・feature flag・未実装budget modeを製品に入れない。将来使う判断をするならnative actual-row cold/resize/reset/splice p95≤8ms、warm≤1ms、全frame≤16.7msを先に測る。今回のmergeはこの将来評価を前提にしない。

既に実行したofficial gpui-pre0.3.9のCPU draw測定（再実行なし。#1479の調査コメントに記録された過去計測）:

| rows | cold median ms | warm | resize 1000→600 | reset |
|---:|---:|---:|---:|---:|
| 1,000 | 17.656 | 0.717 | 16.132 | 15.413 |
| 5,000 | 87.998 | 2.679 | 83.335 | 77.996 |
| 20,000 | 399.904 | 13.776 | 345.734 | 335.158 |

M1 Pro/CoreText/release、EN/JA diff風rowとhunk/review card、各3fresh context。GPU present/実Kagi E2E/256rowsの測定ではない。`measure_all`後のoffscreen spliceは再arm必要、automatic resizeだけではfraction anchor非保持、`scroll_to_end`成功でも未知heightがあれば`is_scrolled_to_end=None`というlifecycle実測も前報どおり。

### 共有variable ownerの必須契約（fork source copyではない）

`crates/kagi-ui-core/src/variable_list.rs`（新規）を既存pane/tabの寿命で保持。既存 `DiffListLayout` はsource/split/gutter identity、`RowScroll`/RowFocusはnavigation、既存generic `with_vertical_scrollbar` (`render_helpers.rs:791–806`) はpublic `ScrollbarHandle`を再利用。別のfocus/selection managerは作らない。

- source-generation単位のstable row keys、numeric measured/estimated heightsとprefix index、viewport/offset、reading key+intra-row fraction、pending reveal/one-shot End、thumb drag-start extentを一つのownerで管理。visible+overdrawだけElementを構築。全text clone/全row shapingをrenderごとにしない。
- source替えはreset、同source recolor/同selection/unchanged reloadはmeasured cacheを保持。width/rem/font/line-height/split/gutterはmetric generationを変える。
- 同count/offscreen insert/replace/deleteでもaffected heightsをinvalid化。new slotは非zero estimateを持つ。変化前のanchor fractionをcapturedし、変化後のactual row heightへ解決。消えるkeyは最寄りsurvivorへ一貫して移す。
- true Endは**実最終item**からviewportを充たすまで測る。oversized tailはbottomを合わせる。end intentは解決後consumeし、appendで勝手にTail/followを始めない。
- thumb drag中はpointer mappingのextentを固定、interiorはprefix inverse、thumb終端はtrue End intent。単にoffsetをestimated maxへclampするだけでは不可。
- actual official TextViewのfit-content giant postはその一rowの全量layout costを持つ。outer virtualizationだけで巨大単一postのframe budgetを保証しない。全文/Reply到達は維持し、巨大row測定は通常row性能と分けて報告する。

### shared diffの全callers（取り残し禁止）

- source sync `core/diff_list.rs:47–81`、constructor `src/ui/render_helpers.rs:540–541`、sync `606–612`、list `674–721`、keyboard mapping `725–750`。
- Main Diff `src/ui/main_diff_pane.rs:42–43,57–58,69–94,204–214`。
- File History `src/ui/file_history.rs:34–35,62–77,174–175`。
- PR diff `src/ui/pr_mode.rs:71–72,332–333,1765–1781`、PR conflict `101–102,345–346,578–588,1741–1745`。
- Editor WIP `crates/kagi-ui-editor/src/lib.rs:539–540,671–672` / `src/ui/editor_workspace.rs:62–70`。
- Editor History `crates/kagi-ui-editor/src/lib.rs:466–467,648–649,3394–3395` / `src/ui/editor_workspace.rs:86–94`。
- test fixture `src/ui/github_pr_detail_tests.rs:35–36,46–47`。
- selection navigation `src/ui/diff_selection/keyboard.rs:79,165–200`、overlay splice `render_helpers/row_overlay.rs:86–104`、thread source invalidation `pr_mode/threads.rs:49–70,330–337`。
- existing RowScroll `crates/kagi-ui-core/src/keyboard_rows.rs:43–109` にOwned variantを追加する。無関係なnative List variantは残す。

### #1479-S5A — 既存keyboard adapterとPR lane（8 files / 250–450 LOC）

`ListKeyboard::rows/focus_first` (`src/ui/list_keyboard.rs:19–55`) をRowScroll引数へ一度で切替。全caller `branch_picker.rs:49,62`, `home_recent.rs:48`, `pr_lane.rs:238`, `oplog_restore_inspection.rs:83`, `issues_mode/feed.rs:75` を同mergeで変更し、旧overloadは残さない。PR lane owner/new `pr_lane.rs:38,46`、reset `230–231`、render `289` をuniformへ。現在描かれたrangeは既存RowList::as_drawn方式で報告する。

**tests first**: `tests/recovery/pr_lane_keys.rs:41,130,210` にtail/Home/End/Page・window-key変更・zoom・実thumb extentを追加。branch/Homeはargument adapterだけでリスト方式は変えない。

### #1479-S5B — Inspector / restore uniform（10 files / 450–800 LOC）

依存A。Inspector owner/new/reset/splice `inspector_files.rs:62–95`、More末尾navigation `142–145`、render/scrollbar `159,182–187` を変更。既存generated-fold/file pathによるsurvivor focusを保持。restore owner/geometry/wheel/parent reveal `oplog_restore_inspection.rs:60–143,157–158`、native専用E2E返値 `193–200` をuniformへ。

**tests first**: `inspector_history_keyboard.rs:63,153,384,388`, `inspector_file_rows.rs:152`, `oplog_restore_keys.rs:16,232,304–310,553,577`。scroll可能paneは端へ達するeventを自分がconsumeし、次のoutward wheelだけ親へ渡す。no-overflow paneは親へchain。parent reveal後のpointer scrollを引き戻さない。Unavailable/More含む全slotに同height。ADR0214 `342–355` のListState固有説明を更新。

### #1479-S5C — variable owner + 6 diff owner全切替（22 files / 2,250–3,700 LOC）

依存#1479-S0（A/Bとは独立、core keyboard_rows共有編集は順に統合）。new moduleだけを未使用scaffoldとしてmergeせず、全6consumerと同時導入。

**tests first**:
- `core/diff_list.rs:106,132` の同count別source/repaint/gutter/expansionの回帰を移植。`row_overlay.rs:158,180,189` のbase-row対応/countテストは残す。
- new module内unit: prefix inverse、offscreen same-count replacement、splice/delete前方、width fraction、font/zoom/gutter generation、frozen drag extent、one-shot trueEnd/no-follow、oversized tail/source-drop。
- 既存7 native oracleは**全て維持**: `pr_diff_extent_unified` (runner3128), `split`3132, `reflow`3136, `threads`3140, `source_recolor`3144, `same_selection`3148, `editor_diff_extent_unchanged_reload`3152。
- test observer `pr_diff_extent_same_selection.rs:77,189–260`, `editor_diff_extent_reload.rs:243–272` をlive owned measured-height＋実paint boundsへ変更。estimateを測定済heightと偽って返すListState互換testshimは禁止。
- 実thumb interior/end、offscreen review card更新のlegを既存extent filesに追加。20,005 rows、single large wheelでtail、逆wheelでfirst、oversized tail bottom≤1px、source recolor/reclick/reloadでdistant measurement保持を弱めない。

### #1479-S5D — Issue variable geometry統一（12 files / 800–1,400 LOC）

依存A,C。accepted conversation `issue_conversation.rs:76,104–105,166,217,273–350`、thread render `issues_thread.rs:87,127,165–207`、feed owner `tab_view.rs:439,582` / pending owner `463,596–601`、feed splice/paging `issues_mode/feed.rs:177–234` を同ownerへ。

**現pinでgreenを保つため重要**: #1479-S3は準備テストだけなので、この時点では既存forkのgroup/viewport callbackを、新geometry handleへ必要最小限に接続して**現仕様を保持**する。これを新APIとして増設せず、#1479-S6で既存groupごと削除する。#1479-S5D単独でcross-postの負条件を有効化しない。最終selector/rendererを二重実装することとは違い、現行consumerのgeometry交換である。

**tests first**: `issues_pagination.rs:60,109–111,133,150,176,263,281`、conversation selection/lifecycle、`github_text.rs:135,181`。same-count offscreen post/metadata/locale変更、reading PostKey前のinsert/delete、巨大composer preview、最終post/Reply到達、width/zoom、append時no-follow、filter中の無用paginationなし、pending→acceptedのfocus/scroll ownerを追加。

opaque reset callers `github_issue_state.rs:52,90,307`, `github_issues.rs:206,236`, `issues_composer_e2e.rs:109,563` は元のexplicit reset意味を維持できれば文字変更不要。reset-to-startとappendを混同しない。per-post semantic cutoverは#1479-S6で行う。

### #1479-S5 各merge unitの全変更ファイル・filters

#### #1479-S5A — 固定PR laneと既存RowScroll adapter

依存: #1479-S0。**files_count 8**。

- `src/ui/list_keyboard.rs`
- `src/ui/branch_picker.rs`
- `src/ui/home_recent.rs`
- `src/ui/issues_mode/feed.rs`
- `src/ui/operations/oplog_restore_inspection.rs`
- `src/ui/pr_lane.rs`
- `tests/recovery/pr_lane_keys.rs`
- `CHANGELOG.md`

**gate filters**: `pr_lane_keyboard,pr_lane_selected,pr_lane_count,home_recent_rows_keyboard,branch_picker_screen_departure`。

#### #1479-S5B — Inspector・restoreのuniform化

依存: #1479-S5A。**files_count 10**。

- `crates/kagi-ui-core/src/keyboard_rows.rs`
- `src/ui/inspector_files.rs`
- `src/ui/operations/oplog_restore_inspection.rs`
- `src/ui/operations/oplog_restore_card.rs`
- `src/ui/operations/oplog_restore_preview.rs`
- `tests/recovery/inspector_history_keyboard.rs`
- `tests/recovery/inspector_file_rows.rs`
- `tests/recovery/oplog_restore_keys.rs`
- `docs/adr/0214-oplog-panel-and-point-restore.md`
- `CHANGELOG.md`

**gate filters**: `inspector_files_keyboard,file_history_focus_return,inspector_generated_focus_retained,inspector_overflow_keyboard,inspector_file_row_hit,inspector_scale,oplog_restore_`。

#### #1479-S5C — 共有variable geometryと全diff owner切替

依存: #1479-S0。**files_count 22**。

- `crates/kagi-ui-core/src/variable_list.rs`（新規、実consumerと同時）
- `crates/kagi-ui-core/src/lib.rs`
- `crates/kagi-ui-core/src/diff_list.rs`
- `crates/kagi-ui-core/src/keyboard_rows.rs`
- `crates/kagi-ui-editor/src/lib.rs`
- `src/ui/render_helpers.rs`
- `src/ui/render_helpers/row_overlay.rs`
- `src/ui/main_diff_pane.rs`
- `src/ui/file_history.rs`
- `src/ui/editor_workspace.rs`
- `src/ui/pr_mode.rs`
- `src/ui/pr_mode/threads.rs`
- `src/ui/diff_selection/keyboard.rs`
- `src/ui/github_pr_detail_tests.rs`
- `tests/recovery/pr_diff_extent.rs`
- `tests/recovery/pr_diff_extent_invalidation.rs`
- `tests/recovery/pr_diff_extent_same_selection.rs`
- `tests/recovery/editor_diff_extent_reload.rs`
- `tests/recovery/pr_detail_adaptive.rs`
- `docs/adr/0222-diff-list-estimated-height.md`
- `docs/adr/0209-pr-review-threads-on-diff.md`
- `CHANGELOG.md`

**gate filters**: `pr_diff_extent_,editor_diff_extent_unchanged_reload,diff_keyboard_selection,split_diff_keyboard_order,diff_selection_revision,diff_hunk_buttons,pr_threads,review_hunk_full,pr_detail_adaptive,pr_detail_lifecycle`。

#### #1479-S5D — Issue feed・accepted/pending conversation geometry

依存: #1479-S5A, #1479-S5C。**files_count 12**。

- `crates/kagi-ui-core/src/variable_list.rs`
- `crates/kagi-ui-core/src/keyboard_rows.rs`
- `src/ui/tab_view.rs`
- `src/ui/issue_conversation.rs`
- `src/ui/issues_thread.rs`
- `src/ui/issues_mode/feed.rs`
- `tests/recovery/issues_pagination.rs`
- `tests/recovery/issue_conversation_selection.rs`
- `tests/recovery/issue_conversation_lifecycle.rs`
- `tests/recovery/github_text.rs`
- `docs/adr/0198-read-only-issues-workspace.md`
- `CHANGELOG.md`

**gate filters**: `issues_pagination,issues_row_secondary_text,issue_conversation_,issue_body_zoom,review_hunk_full`。

**Zed patch除去は#1479-S5単独unitにしない**。最後の#1479-S6でroot `Cargo.toml:181–205`（22 entriesは184–205）とlockをofficial package familyへ同時変更する。古いline番号181–202という前snapshot由来の値は使わない。#1479-S5A〜#1479-S5Dのchurn計は `[INFERENCE]` 3,750–6,350 LOC、同一ファイル重複あり。


## 投稿 Copy ボタンの調査結果 — 「既存ボタンで代替」はできない

**現c0a4ea3には、Issue/PR投稿本文全体のCopyボタンがない。** UI全体のCopy関連検索に加え、実際の全投稿rendererを確認した。

| 投稿 | 一次根拠 | 結果 |
|---|---|---|
| Issue本文/コメント | `src/ui/issues_thread.rs:214–276`、processor `93–112` | meta/title/TextView/state/avatarのみ。投稿Copy無し。 |
| PR description | `src/ui/pr_conversation.rs:25–99`、本文 `82–89` | 投稿Copy無し。 |
| PR review/comment/line-comment | `src/ui/pr_conversation.rs:228–334,509–529` | 本文Copy無し。diff_hunkのあるreview commentだけ次行の別機能あり。 |
| PR review hunk | `src/ui/pr_conversation/hunk.rs:27–40,94–126,128–180`、caller `pr_conversation.rs:309` | **Copy this hunk / このコードをコピー**。raw `ReviewComment.diff_hunk`全量専用。投稿本文ではない。 |

hunk buttonのIDは `("pr-convo-hunk-copy", number*1000+i)`、clickは表示時の `hunk.full.clone()` をclosureに捕捉してCopyする。stable GitHub comment IDから本文を再検索するcommandではない。collapse表示12行でもfull hunkをCopyする点は保持する。ただしボタン自体はvirtual feedのrowがmountedのときだけ存在し、画面外任意投稿のglobal commandではない。

Issueには `PostKey::Comment(opaque ID)` (`issue_conversation.rs:26–28,252–259`) があるが、それをtargetにする本文Copy actionは実装されていない。PRのflattened Entry (`pr_conversation.rs:126–151`) はcomment IDを持たず、index-based presentation。存在しないstable-ID buttonを移行済と記載しない。

**本計画の代替**: 目的の投稿まで戻り、mounted TextViewの公式SelectAll/Copyを使う。raw GitHub Markdownではなく、既存PreparedBody→公式Plain-rendered selection→Copyのtrimという経路。外側unmount済の単一投稿をキーボードcommandだけで全文取得する保証は持たない。

投稿全体のCopyボタン新設は、ownerが追加scopeとして望む場合のみ別途設計する（raw Markdown / prepared Markdown / rendered plainのどれをCopyするか、stable full owner identity、画面外targetの選択方法を決める必要あり）。**今回は未依頼の新機能として追加しない**。hunk Copyを本文Copyと誤表示して代用もしない。


## U1 — 最小の公式Textarea accessor（外部prerequisite、選択用U0とは別）

追加調査で公開APIの代案まで確認した。

- **shrink/internal scroll側は既存public APIで足りる**: `TextareaState::new(...).rows(1)` と `Textarea::h(relative(1.))`。外側をKagiがpreferred height / min_h(0) / flex_shrink(1)にすれば、現composerと同じ役割分担にできる。
- 不足はpreferred3–12行を決める**既存cached visual rowsへのread-only queryだけ**。official `InputPresentation` (`base/input/base/state.rs:564–625`) にrows/height/scroll extentなし、`631–650`はcaret/viewportのみ。`scroll_handle` / `scroll_size` (`488,496`) はcrate-private。`display_map()/prewrap()` (`11836–11865`) はEditorMode限定。
- Kagiにpublic `DisplayMap` をもう一個持つ代案自体は可能。しかし source/幅/font変更時に同じ本文を二重wrapping/shapingする。公式wrapperはUnicode break/grapheme・shaped prefix幅を使う (`base/input/editor/display_map/text_wrapper.rs:32–111,365–435,525–614`)。単なる文字数/GPUI LineWrapper代用は同じ行数を保証せず、12行clampしても第二mapの全logical-line処理量はboundedにならない。不要な計算/コピーと長期のwrap意味同期を持ち込むので採らない。
- auto_growだけではleaf minがpreferred rowsになるため、現仕様どおりの縮小を阻害する。getterのためにcode EditorStateを使うとfont/indent/folding/search/keycontextまで増え、これも採らない。

**確定するupstream提案**: TextareaStateに既存cacheの `wrap_row_count()` を返すO(1) getter（提案名 `visual_row_count` 等）を追加。frame前/幅変更後のcache更新タイミングをdoc化。別height-policy API/新rendererは不要。

**upstream変更3ファイル / [INFERENCE] 80–180 LOC**:
1. `crates/base/src/input/base/state.rs:11660–11718`（小accessor、colocated unit）
2. `crates/kit/tests/integration/input/textarea.rs:476–535,1120–1137,1208以降`（plain rows1＋Kagi式preferred/shrink layout、soft wrap/Unicode/resize/内部scroll）
3. `website/component/textarea.md:30–42,86–105`（auto-grow minimumとapplication-owned preferred heightの違い）

**依存・merge条件**: officialに正式mergeされたrevを#1479-S6で採用する。それまでは#1479-S0〜#1479-S5を進められるが、全要件を保持した最終#1479-S6はmergeしない。API名/採用rev/acceptanceは未確定であり、調査中にPRは作成していない。拒否された場合、二重map・仕様縮小・forkを無断採用せずownerに差分を提示する。

## #1479-S6 — official package family + typed APIs + group撤去の原子的切替

**依存**: #1479-S1/#1479-S2/#1479-S3/#1479-S4/#1479-S5A〜#1479-S5D と U1のofficial取り込み。**一つのgreen merge unit**。新base/typed APIと依存identityが同時に変わるため、manifestだけ、Findだけ、Radioだけ、旧groupだけという赤い中間commitをdevへmergeしない。PR内の作業/レビューは以下のsubsectionごとに行えるが、mergeは一回。

### A. 7 manifest / 一つのGPUI identity / zero patch

| manifest:line | 変更 |
|---|---|
| `Cargo.toml:23,39–51,140–146,181–208` | gpui/pre-platform、official component＋base＋kit-assets、feature forwarding、profile名、残patch全削除。 |
| `crates/kagi-ui-core/Cargo.toml:17–18,36` | gpui-pre/official component、Linux testのgpui-pre-wgpu。 |
| `crates/kagi-ui-ecosystem/Cargo.toml:17–18` | 同一source/rev。 |
| `crates/kagi-ui-editor/Cargo.toml:13,18–19` | 同一source/rev、test-support forwarding維持。 |
| `crates/kagi-ui-file-history/Cargo.toml:19–20` | 同一source/rev。 |
| `crates/kagi-web/Cargo.toml:20,23–24,31` | gpui-pre / pre-web / pre-platformのtarget別指定。 |
| `vendor/gpui-terminal/Cargo.toml:49–57` | owned terminalも同じgpui-pre family。 |

**既にresolve実証した形**（#1479調査コメントの独立dependency resolve実験）:

```toml
gpui = { package = "gpui-pre", version = "=0.3.9" }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.9", features = ["font-kit", "x11", "wayland", "runtime_shaders"] }
gpui-component = { git = "https://github.com/longbridge/gpui-kit", rev = "1188c11003b5cb5b83bc07d3fb9112e7674fbfa7", features = ["tree-sitter-languages"] }
gpui-kit-assets = { git = "https://github.com/longbridge/gpui-kit", rev = "1188c11003b5cb5b83bc07d3fb9112e7674fbfa7" }
```

これがそのままU1を含むという意味ではない。**最終revはU1を取り込んだofficial実SHAへ全manifest一斉更新**。base Radio/`TextSelection::clear`のpublic seamを使うrootには同じofficial revの `gpui-base` を直接追加する（component互換facadeに存在しない `gpui_component::base` というimportを発明しない）。既存 `gpui_component::...` の多くはofficial component facadeでそのまま残せる。Kit facadeへ全namespaceを機械置換する理由はない。

- `Cargo.lock` 再生成。#1479-S1でcosmic削除済、#1479-S6でZed sibling **22 entries（184–205）** とcomponent/assets **2 entries（207–208）**、空patch headers/commentsを全削除。
- `[profile.dev.package] gpui` は `gpui-pre`。`gui-e2e`以外の通常buildでtest-supportが無条件に入らない条件を維持。
- `src/ui/assets.rs:253,262`: `gpui_component_assets::Assets` → `gpui_kit_assets::Assets`。
- main/E2E/Settingsの `Root::new(...).font(ui_font())` はpublic Styledのfontメソッドで成立するので、Root全面置換は不要（Kagi `mod.rs:3516`, `e2e.rs:918`, `settings_window.rs:35`）。

### B. typed Input / Textarea / Editorの全主要境界

fresh grepでmulti_line builderは**4か所（3種類のstate、Commit再生成が2回）**、code_editor builderは**3か所**。single-line InputStateを全置換しない。

| Kagi file:line | exact migration |
|---|---|
| `src/ui/commit_panel.rs:432,585–605` | bodyだけEntity<TextareaState>。titleはInputState。両方をInputState固定closureへ渡すreadを型別に分ける。 |
| `src/ui/operations/commit.rs:108–114` / `commit_panel_composer.rs:173–181` | body作成/locale再作成をTextareaState::new().rows(1)。draft/template/set_value/replace_all ownershipは変更なし。 |
| `commit_panel_composer.rs:56–61` / `compact.rs:128` | 異なるentity型を同じ配列に入れる処理を分離。focus判定はFocusHandleへ射影して既存判定を再利用。 |
| `commit_panel_composer_render.rs:28–35,87–91` | U1 getterのcached行数clamp3–12をpreferredに使用。plain Textarea::h(relative(1.))、元のmin/shrink/clip/focus-ring/footerを維持。 |
| `src/ui/pr_mode.rs:988–991` / `mod.rs:1226` / `pr_page.rs:577` | PR本文composer→TextareaState/Textarea、auto_grow2–8はそのまま。 |
| `src/ui/settings_view.rs:66–68,180,1133` / `mod.rs:1219` | analyze ignore→TextareaState/Textarea。setter/save/resetのwriter契約は変更なし。 |
| `src/ui/issues_composer.rs:36,79–82,185,489` | Issue body→EditorState::new().language(...)/Editor、titleはInputState。 |
| `src/ui/conflict_view.rs:70,1020,1092–1095` / `mod.rs:1411` / `conflict_editor.rs:1567` | Conflict result→EditorState/Editor。language/read-only/busy/approvalは既存値を保持。 |
| `crates/kagi-ui-editor/src/lib.rs:276,495,1537–1540,1592,3300` | open buffer/history editor state/construct/render→EditorState/Editor。historical selectable TextViewは別のまま。 |
| `src/ui/timeline_row.rs:201–205` | `body_input`を型隠蔽enumにせず、PR Textarea / Issue Editorの薄いrender entryへ分ける。callerは上記2か所。 |
| `src/ui/issues_composer_e2e.rs:185,289–290,395–398` | 返り値とtitle/body hetero iteratorをtypedに直す。fake統一InputStateを残さない。 |
| `src/ui/window_menu.rs:67–68,131–132` / `src/ui/render.rs:246–247` | focused_inputがAnyInputStateになるので `.read(cx).focus_handle(cx)` ではなくpublic `.focus_handle(cx)`。`modal_key_routing.rs:54–55` は既にこの形で原則不変。 |
| #1479-S4の `core/input_actions.rs` | AnyInputStateの3variantsをpublic enumでmatch、同じIME capture/bubble契約を保つ。 |
| `tests/recovery/editor_find.rs:6,22ほか`, `editor_find_hosts.rs:7,14`, `editor_snapshot_readonly.rs:23,92`, `menus_ja.rs:144`, `commit_panel_viewport.rs:188` | testsのtyped境界とgetterを対応。single-lineのref-validation/remote input等は変更しない。 |

**composer replacement sketch（getterはU1の公式確定名に合わせる）**:

```rust
// State: no auto-grow minimum. Textarea keeps editing/wrapping/scrolling ownership.
let body = cx.new(|cx| TextareaState::new(window, cx).rows(1));
// After upstream U1: query its existing cache, do not shape a second text copy.
let rows = body.read(cx).visual_row_count().clamp(3, 12);
let preferred = px(rows as f32 * theme::rem_size_px() * 1.25)
    + gpui_component::Size::Medium.input_py() * 2.;
div().h(preferred).min_h(px(0.)).flex_shrink(1.).overflow_hidden()
    .child(Textarea::new(&body).h(relative(1.))
        .aria_label(Msg::A11yCommitDescription.t())
        .appearance(false).bordered(false))
```

コード全体のcompile済例ではなく、既存composer frame内の置換sketch。3/11/soft-wrapped6行の現oracle（95/239/149px、ADR0134:200–223）と低window内部scrollを受入で照合する。

### C. Find / Radio / Select / Theme / Undo

- **Find**: `core/find.rs:1–17` を既存labels ownerのままKagiの薄いFind UIへ拡張。official Editor `.searchable(false)` でbuilt-in chromeだけ使わず、公開engine `search_session()`136、`set_search_query()`172、`close_search()`186、`next_search_match()`191、`previous_search_match()`200、`replace_current_search_match()`211、`replace_all_search_matches()`244（`base/input/editor/search.rs`）を使う。regex/match index/undo backendを二重実装しない。
  - 3 host: Editor、Conflict Result、Issue body。`search_labels`3builder＋`set_search_labels`3更新を全撤去。
  - 2 named Inputs＋7 named Buttons、既存EN/JA Msg、tooltip、replace-mode/case state、Tab/Shift-Tabの実順、Escape→buffer、padding click→Find focus、Button上のediting actionがhidden editorへ流れない境界を維持。
  - upstream `cycle_focus`はreplace mode時の2入力間だけで、button traversal/labels/help/containmentの代替ではない。
  - `tests/recovery/editor_find.rs:22–28,73` のfork SearchPanelProbe/help-probeを削除。#1479-S4のnative AX /実focus /actual button bounds /hoverで呼ばれたTooltipを使う。official提供のtest queryを使う場合も実Element snapshotであり、名前定数の自己比較にしない。
- **Radio**: `settings_view.rs:699,713,750,965` の4groupを **新規** `src/ui/settings_radio.rs` の薄いKagi paintへ。official `gpui_base::RadioGroup` / `Radio` を使い、checked+disabled+AX+pointer/Enter/Spaceはprimitiveへ委譲。indicatorだけ現6/14px dotとprimary_foreground。既存4groupのcallbacksとstable element/focus IDsを維持。base RadioGroupはsemantic containerであり、存在しないautomatic roving-arrow機能を前提にしない。新しい設定storeや独自Radio event systemは作らない。
- **Select**: `settings_view.rs:503–520` のbuilderは `.accessibility_label(...)`。actual focused ComboBoxのrole/value/openはupstreamが所有。既存 `ThemeOption::title` のsanitize (`99–108`)を残し、AXだけ別raw名にしない。custom selection state/open flagを追加しない。
- **Theme**: `core/theme.rs:1271,1277` のdirect `global_mut` は新base projection/tokensと乖離する。official `Theme::update(cx, |theme| ...)` (`component/theme/mod.rs:255–289`) を使うか、同等に全projectionを同期。`sync_base`だけではtokensはreconcileされないことにも注意。選択active色が同値の毎renderには更新しない（不要clone/refresh-loopを防ぐ）。色変更時のみreconcile+Base sync。全preset/selected/inactive/scrollbar/radio contrastを維持。
- **Undo**: `commit_panel_composer.rs:99,113`、`issues_composer_e2e.rs:235,255,290` 等の `replace_all`は公式atomic groupingのまま。set_valueへの置換は禁止。`smart_replace_undo` は前後typingを含めsleep無しでUndo/Redo境界を保持。
- **Lifetime**: fork b50aの旧strong next-frame経路はofficialにないが、`pr_detail_lifecycle` / `smart_generation_close_drops_panel` のnative release/orphan assertionsを実行して初めて移行完了とする。

### D. SDK group撤去とKagi既存ownership guardの整理

**設計を一つに絞る**: SDK `TextSelectionGroupRetirement` / Root group / all-post members / outer edge taskは削除。ただし既存Kagi `ConversationScope` の **active/generation** はaccepted-row/deferred-render validityにも使われるので残す。これをofficial `TextSelectionScopeId`に置き換えない。新しいcross-post model/parser/canonical byte cacheは作らない。

- `issue_conversation.rs:40–55` のSDK retirement field/呼出しを除去、`:89–95` のSDK lease active check、`:106,136–144,277–284` のgroup作成・publish、weak Root installation/defer部を除去。PostKey/source/retained TextView/geometry/acceptance世代は維持。不要になるsource_revisionは消す。
- `issues_thread.rs:3,135–158` のgroup viewport marker/held-edge callbackを削除。TextView `237–245` は公式の通常selectable/default scope。PR側の `timeline_row::body_markdown` / PR description /review/thread/previewも公式挙動を禁止するscopeを付けない。
- **source差替え**: `issue_conversation.rs:342–350,389–395`、削除post判定 `223–225` で旧選択をpublic `TextViewState::clear_selection` と必要なwindow clearで同期撤去してからtextを更新。clearをstate.updateの内側から同stateへ再入させない。refresh開始/失敗/無関係なpost更新で無条件全clearしない。
- **古いpaint済routeからのCopy/SelectAll**: `src/ui/render.rs:690–703` の既存Kagi root action境界にcaptureを置き、paint時に捕捉した**既存Kagi owner/generation**がまだforeground/activeかを現在のapp状態で確認する。staleならpublic `gpui_base::TextSelection::clear` /必要に応じ `clear_for_window` で撤去し、そのactionをconsume。validなら一切加工せずofficial descendant/Rootへpropagateする。新しいselection scope/renderer/clipboard formatter/全post scanは作らない。
- 捕捉するのは既存Scope scalarのRc/weak lifetimeであってIssueConversation/Input entityのstrong lifetimeではない。native inputが現在のvalid foregroundを持つ場合は通常どおりInputが先にCopy/SelectAllをconsume。modal/departed画面の古いInput routeを「現在のInput」と誤認して許可しない。
- **このcaptureを実証する必須ケース**: post focus/Reply focus/Kagi root focusそれぞれから、same-update departure→Copyとmodal表示→Copy。Root fallbackがcaptureを迂回するならgate失敗であり、no-App Dropを扱うhidden leaseを再導入してごまかさない。owner-close/retarget/source mutationの実product eventから検証する。
- 任意のno-App DropだけでSDK global内部stateが即空になるというfork固有保証は撤去。observable Copy/SelectAll actionと、公式actual-unmount frame sweepを受入対象にする。この区別をlifecycle testとADRで明記する。

**既存retire/activation callersはgeneration ownerとして意図的に維持（API shimではなく既存accepted lifecycle）**:
`github_issue_state.rs:84,231,244,292,322–342`; `github_issues.rs:158,175,190,249,277`; `issue_conversation.rs:457,482,496,532,579`; `issues_composer.rs:260,262`; `branch_cleanup.rs:49`; `command_palette.rs:332`; `ecosystem.rs:90`; `file_history.rs:241`; `overlay_focus.rs:366,410`; `pr_mode.rs:264`; `tabs.rs:217`; `workspace_mode.rs:635,745`; `operations/commit.rs:900`; `operations/conflict_detect.rs:288`; `operations/modal_state/arbitration.rs:48,66`。

この設計ならpure/no-cx modal setters全体にAppを注入する大改造は不要。`depart_active_tab` (`tabs.rs:204`, callers `267,399`, `home.rs:85,111`)、`leave_takeovers` (`workspace_mode.rs:621`, callers `673,680,707,727,760`)、`leave_settings_for_overlay` (`overlay_focus.rs:365`, callers `about.rs:14`, `commands.rs:2662,2674`) も既存generation失効の入口として残す。`tab_view.rs:458–464,593–595` / `tab_ui_state_ops.rs:148–150` のKagi lifetime holderと `github_issue_state.rs:322–342` のコメントはaccepted-generation guardという用途に明確化し、SDK group storageとしては残さない。

#1479-S3の13旧scenario改訂と5新規positive scenario、ADR0198/0136/0200/0142・decisions・CHANGELOG・verify skillを同mergeで切替。未変更のretirement callerを「SDK group caller取り残し」と誤判定しない一方、SDK型/import/methodのliteralがproduction/testに残らないことは確認する。



### #1479-S6 regressionの先行順・最終gate

1. #1479-S3正条件を公式as-is oracleへ更新する前に、旧跨ぎ/画面外保証を削る箇所を上記13行と照合する。source/privacy/Input/repo不変のassertは削除しない。
2. #1479-S4の実AX/Input/IME、#1479-S5の実geometryをbaselineとして使う。typed Input移行のcompile対応でtest oracleそのものを文字数近似や宣言定数へ変えない。
3. Findは2inputs+7buttonsのnative role/name/help/focus/state/keyboard contaminationを先に書く。Radioはdot/foreground contrastとpointer/Enter/Space、Selectはactual ComboBox。Themeはbase projectionも含む実描画を比較。
4. composerはU1のofficial integration testに加え、Kagiの既存 `commit_panel_viewport` の3/11/soft-wrapped6行、native height95/239/149px、低window・high zoomで内部wheel/入力/Commit footerを維持。font shapingをmockしない。
5. selectionのold no-App SDK lease観測を、実product transition→Copy actionとofficial completed-frame unmountへ改訂。根拠なくnative失敗をskipせず、Root capture routingを修正する。

**#1479-S6の絞込みgate（全部実在するprefix、括弧の新規scenarioを除く）**:

`issue_conversation_,pr_conversation_post_select_all,pr_thread_post_select_all,markdown_preparation_edges,workspace_mode_toolbar,pr_same_number,pr_threads,review_hunk_full,issue_body_zoom,graph_copy_foreground,graph_pointer_focus,editor_,conflict_,settings_,sidebar_search_,input_names_inventory,input_ime_escape_boundaries,commit_panel_,smart_replace_undo,smart_generation_,pr_detail_lifecycle,pr_diff_extent_,diff_keyboard_selection,split_diff_keyboard_order,diff_selection_revision,terminal_,status_bar_,finder_scale_rest,inspector_scale,oplog_detail_select_copy`

新規は #1479-S3/#1479-S4節に列挙したscenarioだけ。#1479-S6はframework family全体が変わるため、絞込みだけで完了せず、通常の**全native suite**（canonical verify skillに従い、明示filterの安全な分割batchで全scenarioを網羅する。unfiltered実行は禁止）と別実行のinventory smoke/比較、workspace testsを通す。inventoryとordinary filterは混ぜない（verify skill:2702–2706）。

**platform/dependency gate**:
- macOS default / gui-e2e build、workspace unit/integration、上記native GUI。leak assertionをoffにしない。
- Linuxの実target build＋#1479-S1のCosmic real-shapeとnative CJK matrix。macOS結果をLinuxの証拠にしない。
- Windowsは対応runner上の実target compile、WASMは既存 `scripts/build-web.sh` と既存web harness。macOS上の無理なcross-link失敗を無視して全platform成功としない。
- `cargo metadata` / `cargo tree --target all --edges normal,build -i gpui-pre` でactive GPUI identityが一つ、root/crate/CLI-configにpatch/replace/local-forkの隠し経路がないこと。optionalな非active `gpui-fast` metadata nodeの存在だけをduplicate active dependencyと誤認しない。
- 全manifestsの `[patch]` ゼロ、lockにTomiXRM/zed・cosmic・gpui-kit fork sourceゼロ、official component/base/assets rev一致。pathのowned terminalは認めた例外として明示。
- checksum付きログ、native screenshot/AX/clipboard/geometry、Linux font identity evidence、実行コマンド・対象SHAを残す。独立resolveの855 packages成功はKagi compile成功の代わりではない。

### #1479-S6 全変更予定パス（distinct 60）

以下は上記設計で変更するパスを数えたもの。新しいAPIを導入するために無関係な全InputState参照を機械変更する一覧ではない。compile未実施なのでcompiler-drivenな小差分の予備は後述に別計上する。

- `src/ui/issue_conversation.rs`
- `src/ui/issues_thread.rs`
- `src/ui/render.rs`
- `tests/recovery/issue_conversation_selection.rs`
- `tests/recovery/issue_conversation_lifecycle.rs`
- `tests/recovery/issue_enabled_input_copy.rs`
- `tests/recovery/issue_offscreen_select_all.rs`
- `tests/recovery/issue_conversation_support.rs`
- `tests/gui_e2e_runner.rs`
- `tests/recovery/conversation_official_selection.rs`（新規）
- `docs/adr/0198-read-only-issues-workspace.md`
- `docs/adr/0136-pr-merge-and-review-conversation.md`
- `docs/adr/0200-pr-workspace-layout.md`
- `docs/adr/0142-markdown-image-rendering.md`
- `docs/decisions.md`
- `CHANGELOG.md`
- `.claude/skills/verify/SKILL.md`
- `src/ui/github_issue_state.rs`
- `src/ui/tab_view.rs`
- `src/ui/tab_ui_state_ops.rs`
- `Cargo.toml`
- `Cargo.lock`
- `crates/kagi-ui-core/Cargo.toml`
- `crates/kagi-ui-ecosystem/Cargo.toml`
- `crates/kagi-ui-editor/Cargo.toml`
- `crates/kagi-ui-file-history/Cargo.toml`
- `crates/kagi-web/Cargo.toml`
- `vendor/gpui-terminal/Cargo.toml`
- `src/ui/assets.rs`
- `crates/kagi-ui-core/src/theme.rs`
- `crates/kagi-ui-core/src/find.rs`
- `crates/kagi-ui-core/src/input_actions.rs`
- `src/ui/commit_panel.rs`
- `src/ui/commit_panel_composer.rs`
- `src/ui/commit_panel_composer_render.rs`
- `src/ui/compact.rs`
- `src/ui/mod.rs`
- `src/ui/settings_view.rs`
- `src/ui/conflict_view.rs`
- `src/ui/conflict_editor.rs`
- `crates/kagi-ui-editor/src/lib.rs`
- `src/ui/window_menu.rs`
- `src/ui/settings_radio.rs`（新規）
- `tests/recovery/editor_find.rs`
- `tests/recovery/editor_find_hosts.rs`
- `tests/recovery/editor_snapshot_readonly.rs`
- `tests/recovery/menus_ja.rs`
- `tests/recovery/commit_panel_viewport.rs`
- `docs/adr/0001-gpui.md`
- `docs/adr/0006-gpui-component.md`
- `docs/adr/0080-settings-window.md`
- `docs/adr/0134-simpler-commit-pane.md`
- `docs/adr/0125-apple-hig-themes.md`
- `docs/adr/0222-diff-list-estimated-height.md`
- `src/ui/timeline_row.rs`
- `src/ui/pr_mode.rs`
- `src/ui/pr_page.rs`
- `src/ui/issues_composer.rs`
- `src/ui/issues_composer_e2e.rs`
- `src/ui/operations/commit.rs`

**#1479-S6 churn見積り**: `[INFERENCE]` **4,200–6,900 LOC**（typed API/Find/Radio/theme/assets/selection/tests/docs、generated lock除外）。そのうちgroup撤去/既存guard整理/oracle改訂は約1,200–2,000 LOC。旧案の38-file App-aware setter全面移行は採らず、既存Kagi validity tokenを実action境界で検証するため、no-cx modal setter chainの100か所規模の引数変更をscopeへ持ち込まない。別途、未コンパイルの公式API差分に対する予備 **0–20 files / 400–900 LOC** を想定する。この予備を確定変更・検証済として数えない。

**最大リスク**: U1のupstream accept待ち、typed APIを一つでも残すこと、Rootのaction propagation/Fallbackとretired ownerの関係、official Markdown/Copyの分離記号/trimを旧fork canonical streamと同一視すること、Theme base/tokensの同期漏れ。U0や独自rich rendererの再導入は解決策にしない。


## 仕上がりの順序・merge単位・依存関係

| unit | merge対象 | depends_on | files_count | churn LOC（lock除外） |
|---|---|---|---:|---:|
| #1479-S0 | Kagi方針/出自 | — | 6 | 90–160 |
| #1479-S2 | Kagi-owned terminal ADR/notice | #1479-S0 | 6 | 110–190 |
| #1479-S1 | native fallback + cosmic patch除去 | #1479-S0,#1479-S2 | 24 | 550–950 |
| #1479-S3 | mounted単一投稿の正条件準備 | #1479-S0 | 4 | 250–400 |
| #1479-S4 | Clear/IME/native AX依存準備 | #1479-S0 | 15 | 450–750 |
| #1479-S5A | PR lane uniform / RowScroll API | #1479-S0 | 8 | 250–450 |
| #1479-S5B | Inspector/restore uniform | #1479-S5A | 10 | 450–800 |
| #1479-S5C | shared variable list / 全diff | #1479-S0 | 22 | 2,250–3,700 |
| #1479-S5D | Issue feed/conversation geometry | #1479-S5A,#1479-S5C | 12 | 800–1,400 |
| U1 | official Textarea cached accessor | —（upstream） | 3 upstream | 80–180 |
| #1479-S6 | official family＋全typed API＋selection最終 | #1479-S1,#1479-S2,#1479-S3,#1479-S4,#1479-S5A〜#1479-S5D,U1 | 60 | 4,200–6,900 |

推奨Kagi merge順は **#1479-S0 → #1479-S2 → #1479-S1 → #1479-S3 → #1479-S4 → #1479-S5A → #1479-S5B → #1479-S5C → #1479-S5D → #1479-S6**。U1は#1479-S0後すぐ別upstream作業として進められるが、取り込みまでKagiは現pinのgreenを保つ。#1479-S5CはA/Bとは独立して先行可能。shared file編集はmerge/rebaseで一人が統合し、別々に半分のAPIをmergeしない。

#1479-S6以外の各unitは現依存pinで独立greenにできる範囲だけを採る。#1479-S3の「準備」と#1479-S6の「保証撤去＋official化」を区別してchangelogに記す。#1479-S5Dはまだ旧group consumerを維持し、#1479-S6で最終撤去する。移行完了後に旧path/flag/aliasを残す計画ではない。

## 未確認事項・完了を妨げる条件

1. **U1のofficial採用SHA/API名は未確定**。公開APIを調べた上で残った最小prerequisite。Kagiをforkで先に動かして既成事実にしない。
2. Kagi全体は本調査でcompile/runしていない。#1479-S6の追加compile調整、native selection/IME/Find/Radio/Themeの統合結果は未検証。依存resolve成功だけをgreenの証明にしない。
3. variable-list新実装の実Kagi latencyは未測定。過去measure_all数値と将来gateを明確に分けた。production measure_all採用は0であり、測っていない256rowを性能根拠にしない。
4. Linux CJKは**これから#1479-S1/#1479-S6で実施するgate**。前調査はコードの経路確認と既存テストの特定であり、この追補でLinux画像を取得したとは主張しない。
5. **投稿全体Copyボタンは存在しない**。画面外任意投稿を既存stable-ID Copyで代替できるという前提は否定済。今回の仕様はmounted公式操作であり、新ボタンは別のowner scope決定が必要。
6. official mounted cross-viewは禁じない。その結果が将来upstreamで変わってもKagi独自の全会話契約として固定しない。変更しないprivacy/accepted-source/Input/draft/repository invariantsとの境界は上記oracle表を基準とする。
7. no-App DropによるSDK内部state即時失効は旧fork固有保証として撤回。実際のforeground transitionからのstale Copy/SelectAllをKagi public action境界で防ぐ設計は、既存same-update native regressionsで必ず実証してからmergeする。

**記録の性格**: 本文は調査時の実装計画をrepo内に保存したものであり、後続sliceの完了報告ではない。過去計測と将来の受入条件を混同しない。各sliceの実行済gateはそのcommit/PRに記録する。
