# GitComet 比較調査

- 調査日: 2026-06-13 / 調査者: research subagent
- 対象: `Auto-Explore/GitComet` shallow clone (`/tmp/kagi-research/gitcomet`, `git clone --depth 1` 成功)
- バージョン: `0.1.15`(`Cargo.toml` `workspace.package.version`)、edition 2024、rust 1.95.0
- 関連: ADR-0031(外部コード流用ポリシー)、ADR-0001/0002/0003/0005/0036、既存 research(jj/gitbutler/zed)

## 0. ライセンス原文確認(最重要ゲート)

- **結論: AGPL-3.0-only。ADR-0031 のゲートにより、コード転写は全面禁止(GPL/AGPL 汚染)。採用は Study/Reimplement(概念参照)が上限。**
- 原文確認:
  - root `LICENSE-AGPL-3.0`(34,523 bytes)= FSF 配布の **GNU AFFERO GENERAL PUBLIC LICENSE Version 3, 19 November 2007** 原文(`Copyright (C) 2007 Free Software Foundation, Inc.`)を確認。
  - `NOTICE`: `Copyright (C) 2026 AutoExplore Oy / This project is licensed under the GNU Affero General Public License Version 3 (AGPL-3.0-only).`
  - `Cargo.toml` `workspace.package.license = "AGPL-3.0-only"`、各 crate `license.workspace = true`(`win32-window-utils` も `license = "AGPL-3.0-only"`)。MIT/Apache のデュアルライセンスは**無し**。
- ADR-0031 §2 ゲート適用: AGPL は GPL より強い copyleft(ネットワーク配布でもソース開示義務)。kagi(非 GPL 配布想定)への**コード転写は禁止**。`crates/gpui`(Apache)のような例外 crate も GitComet 側には存在しない(全 crate AGPL)。
- さらに GitComet は **open-core 商用**(README「Editions」: Open Source €0 / Professional €20 lifetime、Claude Code/Codex/GitHub CLI 連携や coverage は Pro)。AGPL + 商用デュアルの典型構成で、流用余地は概念のみ。
- **ハードルール**: GitComet からの関数・型・アルゴリズム実装のコピーは一切不可。設計・UX・データ構造の**考え方**を見て kagi 流に再実装する場合も「原典コードを転写しない」を厳守。

## 1. 環境・依存サマリ(kagi との非互換点)

| 項目 | GitComet | kagi | 影響 |
|---|---|---|---|
| ライセンス | **AGPL-3.0-only**(+商用) | 非 GPL 想定 | **コード流用不可** |
| Git backend | **gix 0.84**(read)+ **git CLI shellout**(mutate/network) | **git2 単一**(ADR-0002) | backend 思想が異なる |
| gpui | **gpui-ce**(Havunen フォーク, git rev pin) | crates.io **gpui 0.2.2 + gpui-component**(ADR-0001/0006) | UI 基盤フォークが別系統、API 非互換 |
| 非同期 | **smol** + 自前 worker thread pool(`thread::spawn`+`mpsc`) | gpui Task / 既存方針 | ランタイムが別 |
| 状態管理 | **Elm/Redux 風 中央 Store**(Msg→reduce→effect→executor、`Arc<AppState>`+`Arc::make_mut` COW) | gpui Entity ベース | アーキ思想が大きく異なる |
| メモリアロケータ | **mimalloc** | (未確認) | パフォーマンス工夫 |
| 構文ハイライト | **tree-sitter**(言語別 feature 多数)+ 自前 alloc crate | (未確認) | diff syntax highlight |
| テキスト入力 | **自前 `kit/text_input`**(gpui `Element` 直実装) | gpui-component の input | gpui-ce にコンポーネント無いため自作 |

注: gpui-ce(community edition フォーク)を pin している点が決定的。kagi の crates.io gpui 0.2.2 + gpui-component とは別系統で、UI コードはそもそも互換性が無い(ライセンス以前にビルド不可)。

## 2. アーキテクチャ比較

### 2.1 Crate 構成(関心分離は良好)

```
gitcomet-core     … ドメイン型 + GitBackend/GitRepository トレイト + 競合解決ロジック(UI 非依存)
gitcomet-git      … NoopBackend(デフォルト)+ トレイト再エクスポート
gitcomet-git-gix  … gix + git CLI による実装(backend.rs / repo/*.rs)
gitcomet-state    … 中央 Store(msg/reducer/effects/executor)= UI 非依存の状態機械
gitcomet-ui       … (薄い)UI 抽象
gitcomet-ui-gpui  … gpui レンダリング(view/panes/panels/rows/kit)
gitcomet          … バイナリ(feature でランタイム選択)
```

- 評価: **core(ドメイン+トレイト)/ state(状態機械)/ ui-gpui(描画)の 3 層分離が徹底**。`GitBackend` がトレイト境界で、デフォルトは `NoopBackend`(feature off 時)→ gix 実装を feature で注入。テスト容易性が高い。kagi も git2 を service 化しているが、GitComet の「state crate を UI から完全分離した Redux store」は設計言語として学べる。

### 2.2 Git backend(gix + CLI ハイブリッド)

- **read 系は gix native**: `repo/log.rs` `history.rs` `status.rs` `diff.rs` `blame.rs` 等。
- **mutate/network 系は `git` CLI shellout**: `gitcomet-core/src/process.rs` の `git_command()` を `gitcomet-git-gix/src/util.rs` の `run_git_*`(`run_git_raw_output` / `run_git_parsed_stdout` …)でラップ。**timeout 付き実行 + cancellation token**(`run_command_with_timeout`, `git_command_wait_poll`)。push/rebase/submodule/tag/remote は CLI(`cmd.arg("--force-with-lease")` 等)。
- README「Requires local Git 2.50+」が裏付け = **CLI 依存が前提**。
- kagi 比較: kagi は **git2 単一 backend**(ADR-0002)で CLI shellout を避け、in-memory merge で repo 無傷を実現。GitComet は **read=gix / write=CLI** の現実主義。利点は CLI の網羅性(submodule, force-with-lease, rebase 等を低コストで提供)、欠点は外部 git 依存・移植性・トランザクション境界の曖昧さ。kagi の純 git2 路線とは思想が逆。

### 2.3 状態管理(Elm/Redux 中央 Store)

- `gitcomet-state`: `Msg`(message.rs)→ `reduce`(reducer.rs)→ `Effect`(effect.rs)→ `executor.rs` の worker pool。
- `Arc<AppState>` を `Arc::make_mut` で copy-on-write(共有参照がある時のみ clone、診断計測付き = `make_mut_state_with_diagnostics`)。
- worker thread は用途別(`default_worker_threads` / `metadata_worker_threads` / `repo_load_worker_threads`)に分離。`repo_monitor`(notify ファイル監視)で外部変更を検知 → Msg 注入。
- 評価: kagi の gpui Entity モデルとは別パラダイム。**「UI から独立した純粋関数 reducer + 副作用は effect/executor に隔離」は設計として学べる**(テスト容易・再現性)。ただし kagi に移植するなら全面再設計で、現状の gpui Entity 路線を覆すコストが大きい → **Study only**。

### 2.4 安全機構(kagi の plan/confirm に相当するものは無い)

- **plan→confirm→preflight→execute→verify→oplog のような統一安全パイプラインは存在しない。** 操作は reducer→effect で**直接実行**。
- 確認 UI は**操作ごとの ad-hoc popover**: `PopoverKind::ForceDeleteBranchConfirm` / `ForceRemoveWorktreeConfirm` / submodule `TrustConfirm` / 認証 prompt 等(`view/mod.rs`)。横断的な「危険操作ゲート」ではなく個別ダイアログ。
- **oplog / undo journal は無し**。`undo_stack` は `kit/text_input/editing.rs` のテキスト編集 undo のみで、**git 操作の取り消しジャーナルではない**。reflog 参照(`reflog_head`)はあるが kagi の oplog とは別物。
- **force 操作は提供**: `push_force` / `delete_branch_force` / `--force-with-lease`(remotes.rs)/ submodule `--force`。kagi の「force 禁止」ポリシー(ADR-0004/0023)とは**正反対**。
- in-memory(無傷)merge 相当も無い。merge/rebase は CLI 実行で working tree を直接変更し、競合は marker で解決させる方式。
- **結論: 安全性は kagi の圧倒的優位領域**。GitComet はパワーユーザー向けに force/直接実行を許容する設計。

### 2.5 競合解決(GitComet の強み領域)

- `gitcomet-core/src/conflict_session/`: marker_parse(`<<<<<<<` 等の解析)+ subchunk(行レベル再 merge)+ **autosolve**(自動解決ヒューリスティクス)+ history。
- **autosolve ルール**(`autosolve.rs`): ① 両辺同一、② base ありで片側のみ変更、③ whitespace-only 差分、④ subchunk 分割再 merge、⑤ **ユーザ定義 regex による自動解決**(`regex_assisted_auto_resolve`、1MiB DFA 上限のサンドボックス付き)。
- UI: `view/conflict_resolver.rs`(4000 行超)+ `rows/conflict_canvas.rs` で 2-way/3-way マージツール(README「2-way and 3-way merge tools」)。
- kagi 比較: kagi は in-memory merge で**競合予測(repo 無傷)**が強み(衝突を起こさずに検知)。GitComet は**実 working tree 上で起きた競合を賢く自動解決**する方向。**両者は補完的**で、autosolve のヒューリスティクス分類(identical/single-side/whitespace/subchunk/regex)は kagi が予測後の解決支援に活かせる設計言語。

## 3. UI 実装

### 3.1 Commit graph 描画

- `view/history_graph.rs`(852 行)で lane layout を自前計算 → `rows/history_canvas.rs` / `history_graph_paint.rs` で canvas 描画。
- データ構造: `GraphRow { lanes_now, lanes_next, joins_in, edges_out, node_col, is_merge }`、`LanePaint { color_ix, incoming, from_col }`、`GraphEdge`。
- **パフォーマンス工夫**: `SmallVec<[_; 3]>`(lane)/ `SmallVec<[_; 2]>`(edge)でヒープ回避、`LaneState.target_ix` を `&str`(40B SHA 比較)ではなく `usize` index で比較(コメントに「40-byte string compare → usize compare」と明記)、`color_ix: u8`、`FxHashMap`、64 色 lane パレットを `OnceLock` で遅延構築。
- kagi 比較: kagi も自前 lane layout(canvas, ADR-0003)。**アルゴリズム発想は近い**(lanes_now/lanes_next の遷移)。GitComet の最適化テク(usize target index、SmallVec inline 容量、u8 color index)は**概念として学べる**(コードは AGPL で転写不可)。

### 3.2 Diff viewer

- inline + side-by-side 両対応(README「Inline and side-by-side diffs」)。
- `view/diff_preview.rs`(`UnifiedDiffLine` trait)+ `view/word_diff.rs`(**word-level diff**、`WORD_DIFF_MAX_BYTES_PER_SIDE = 4KiB` / total 8KiB のサイズガード)+ `rows/diff_canvas.rs` / `rows/diff_text.rs` で canvas 描画。
- tree-sitter による syntax highlight(`kit` 経由)、画像 diff(`diff_file_image`)対応。
- **巨大ファイル対策**(README「Chromium 級で responsive」が動機): word-diff のバイト上限、canvas 描画、diff のキャンセル可能 API(`diff_*_cancellable`)。
- kagi 比較: word-diff のサイズガードと cancellable diff は**性能設計の学び**。

### 3.3 パネル構成

- `view/panes/`: `history` / `main` / `sidebar` / `details`(commit graph + diff + branch sidebar + detail の 4 領域)。
- `view/panels/`: `action_bar` / `bottom_status_bar` / `repo_tabs_bar` / `popover` / `main`。
- **マルチリポジトリ**: `repo_tabs_bar`(kagi の repo tabs ADR-0027 と同コンセプト)。
- 評価: kagi と全体構成(history + diff + sidebar + status bar + repo tabs)は**収束進化的に似ている**。

### 3.4 テキスト入力

- **自前 `kit/text_input/`**: `element.rs`(gpui `Element` 直実装、`TextElement`/`PrepaintState`)+ `editing.rs` / `shaping.rs`(テキストシェイピング)/ `wrap.rs`(折返し)/ `highlight.rs` / `state.rs`。
- undo/redo(`undo_stack`)、選択、wrap cache、可視行レンジ計算など本格実装。
- 理由: gpui-ce フォークに gpui-component 相当が無いため**フルスクラッチ**。
- kagi 比較: kagi は **gpui-component の input** を使用(ADR-0006)。GitComet の自前実装は gpui の Element トレイト直叩きの学習素材だが、kagi は gpui-component で十分なので**実用上は Reject**(学習目的のみ Study)。

### 3.5 テーマ

- `theme.rs`(2196 行)+ `OUT_DIR/embedded_themes.rs`(ビルド時に JSON テーマを埋め込み)。
- `AppTheme { is_dark, colors, syntax, graph_lane_palette(64色), radii }`、JSON で外部テーマ追加可(`docs/themes.md`)、`WindowAppearance` 連動(OS のダーク/ライト追従)。
- kagi 比較: kagi のテーマ機構(ADR-0036、進行中)と**設計が近い**(JSON テーマ + dark/light + graph lane palette + syntax colors)。GitComet の「ビルド時埋め込み + 実行時 JSON ロード」「graph lane を 64 色パレット化」「OS appearance 連動」は**設計言語として有用**(コードは不可)。

## 4. kagi に無い / GitComet 優位な機能(価値順)

1. **競合 autosolve エンジン**: identical/single-side/whitespace/subchunk/**ユーザ regex** の自動解決。kagi の予測型 merge と補完的で価値大。
2. **2-way/3-way マージツール UI**: canvas ベースの本格マージ解決(`conflict_canvas`)。
3. **word-level diff + side-by-side + 画像 diff + tree-sitter syntax highlight**: diff viewer の表現力。
4. **巨大リポジトリ性能設計**: mimalloc、SmallVec inline、usize target index、cancellable diff、worker thread pool、canvas 描画(Chromium 級が動機)。
5. **CLI shellout による操作網羅性**: submodule, force-with-lease, rebase continue/abort 等を低コストで提供。
6. **Elm/Redux 中央 Store**: UI 非依存の純粋 reducer + effect 隔離(テスト容易性)。
7. **JSON 外部テーマ + OS appearance 連動**(成熟)。
8. **multi-OS 配布インフラ**(brew/apt/AUR/Gentoo/MS Store)。

## 5. kagi 優位な点(差別化)

1. **統一安全パイプライン**(plan→confirm→preflight→execute→verify→oplog): GitComet は ad-hoc 確認のみで横断ゲート無し。
2. **oplog / undo ジャーナル**: GitComet に git 操作の取り消し履歴は無し(reflog 参照のみ)。
3. **in-memory(repo 無傷)merge による競合予測**: GitComet は実 working tree を変更してから marker 解決。
4. **force 禁止ポリシー**: GitComet は force-push/force-delete を許容。kagi は安全側に倒す。
5. **git2 単一 backend**(外部 git 不要・移植性): GitComet は git CLI 2.50+ 必須・read/write で gix/CLI 二重。
6. **完全 OSS**: GitComet は open-core(主要連携機能は Pro €20)。

## 6. 提案分類テーブル(ADR-0031 の流儀)

**前提: GitComet は AGPL-3.0-only。全項目で「コード転写不可」。採用は Reimplement(概念再実装)/ Study(記録のみ)が上限。Adopt/Port は構造的に発生しない。**

| # | 項目 | 分類 | 理由 / ライセンスゲート | MVP/later |
|---|---|---|---|---|
| 1 | 競合 autosolve(identical/single-side/whitespace/subchunk/regex) | **Reimplement** | AGPL。ヒューリスティクス分類のみ kagi 流に再実装。kagi の予測 merge と補完的で価値大 | later |
| 2 | 競合解決マージツール UI(2/3-way, canvas) | **Reimplement** | AGPL。UX パターンのみ。kagi の安全機構と統合して再設計 | later |
| 3 | Commit graph lane 最適化(usize target index, SmallVec inline, u8 color) | **Reimplement** | AGPL。kagi の既存 lane layout(ADR-0003)に最適化発想のみ取り込み | later |
| 4 | word-level diff + サイズガード | **Reimplement** | AGPL。アルゴリズム発想 + バイト上限ガードの考え方のみ | later |
| 5 | cancellable diff/status API(CancellationToken) | **Reimplement** | AGPL。巨大 repo 向けキャンセル設計を kagi の git2 経路で再実装 | later |
| 6 | JSON 外部テーマ + ビルド埋め込み + OS appearance 連動 | **Study** | AGPL。kagi テーマ機構(ADR-0036)が進行中。設計言語として記録、ADR-0036 で再評価 | — |
| 7 | Elm/Redux 中央 Store(reducer + effect 隔離) | **Study** | AGPL かつ kagi の gpui Entity 路線と非互換。全面再設計コスト大。設計記録のみ | — |
| 8 | gix + CLI ハイブリッド backend | **Reject** | kagi は git2 単一 + force 禁止 + in-memory merge(ADR-0002/0004)。思想が逆。CLI 依存・移植性低下 | — |
| 9 | 自前 text_input(gpui Element 直実装) | **Reject** | kagi は gpui-component input(ADR-0006)で充足。gpui-ce フォーク前提の実装は不適合 | — |
| 10 | mimalloc / worker thread pool | **Study** | AGPL かつ gpui ランタイム前提が異なる(kagi は gpui Task)。性能ネタとして記録のみ | — |

## 7. 取り込み手順チェックリスト(ADR-0031 §3、代表 = #1 競合 autosolve)

1. ライセンス: **AGPL-3.0-only(原文確認済)→ コード転写不可、概念のみ**
2. 依存 crate: GitComet は gix/smol/regex。kagi は git2。**regex は kagi でも利用可だが実装は転写せず再設計**
3. UI/ロジック分離: autosolve は `gitcomet-core`(UI 非依存)= 概念抽出しやすい
4. 単独 crate 切り出し: 概念のみ抽出のため N/A
5. gpui 統合: ロジックは gpui 非依存
6. 流用 vs 再実装: **再実装一択**(AGPL)
7. MVP or later: **later**(kagi は予測 merge が先)
8. テスト戦略: kagi の fixture repo で identical/single-side/whitespace の各ケースを再現
9. 既存アーキ影響: kagi の in-memory merge → 競合検知後の**解決支援**として autosolve を後段に配置(安全パイプラインの verify 前後)。force 禁止・oplog と整合
10. メンテリスク: 概念再実装のため低。regex autosolve は DFA サイズ上限のサンドボックスを kagi 側でも設ける

## 付録: 確認したファイル(一次資料、/tmp/kagi-research/gitcomet)

- ライセンス: `LICENSE-AGPL-3.0`, `NOTICE`, `Cargo.toml`
- backend: `crates/gitcomet-core/src/services.rs`(GitBackend/GitRepository トレイト), `crates/gitcomet-git/src/lib.rs`(NoopBackend), `crates/gitcomet-git-gix/src/{backend.rs,util.rs,repo/*.rs}`
- state: `crates/gitcomet-state/src/{model.rs,msg/*.rs,store/{mod.rs,reducer.rs,effects/*.rs,executor.rs,repo_monitor.rs}}`
- 競合: `crates/gitcomet-core/src/conflict_session{.rs,/autosolve.rs,/marker_parse.rs,/subchunk.rs}`
- UI: `crates/gitcomet-ui-gpui/src/{theme.rs,view/history_graph.rs,view/word_diff.rs,view/diff_preview.rs,view/conflict_resolver.rs,view/mod.rs,view/panes/*,view/panels/*,rows/*,kit/text_input/*}`
- README/docs: `README.md`, `docs/themes.md`, `docs/shortcuts.md`
