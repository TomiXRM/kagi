# T-WS-EDITOR-003: full worktree tree + エディタモード磨き

- Status: todo
- Group: workspace framework / エディタモード
- 仕様の正: ADR-0120 §Decision 4。依存: T-WS-EDITOR-002。

## 背景

001/002 の tree は「変更ファイルのみ」(データ源が既存の `working_tree_status` のため)。
本チケットで worktree 全体のブラウズと使い勝手を仕上げる。

## スコープ

1. **full tree**: 変更有無に関わらず worktree の全ファイルを tree 表示。
   ignored の除外は自前 .gitignore パースではなく Backend 側の status/ls 系列挙を使う
   (`kagi-git` に read-only の列挙関数を追加してよい。git2 は kagi-git 内のみ)。
   大規模 repo 対策: 遅延展開(ディレクトリ開閉時に列挙)+ uniform_list 仮想化。
2. 変更ファイルには 001 の change バッジ/diffstat を残す(全ファイル表示との合成)。
3. tree → hunk ジャンプ、hunk ヘッダ → エディタ該当行スクロール。
4. tree のフィルタ入力(sidebar の filter パターン踏襲)。
5. モード状態の保持検討: タブ切替で Graph に戻す 001 の暫定を、per-tab 保持
   (選択ファイル・スクロール位置含む)へ昇格するか判断して ADR-0120 に追記。

## 触ってよいファイル

`src/ui/editor_workspace*.rs`, `src/ui/file_tree.rs`(共通データモデルの拡張),
`crates/kagi-git/src/`(read-only 列挙の追加), `src/ui/workspace.rs`, `src/ui/i18n.rs`。

## 触ってはいけないファイル

`crates/kagi-git/src/ops/*` の write 系、既存 `[kagi]` コントラクト行。

## 完了条件

- [ ] 変更のないファイルも tree から開ける(ignored は出ない、遅延展開で大規模 repo でも固まらない)
- [ ] 変更バッジ・フィルタ・tree→hunk→エディタ行ジャンプが動く
- [ ] `kagi-git` の列挙関数に integration test、`cargo test --workspace` 全パス
- [ ] `grep -rE 'git2::|Repository::open' src/ui` = 0
- [ ] GUI 目視は PM

## テスト方法

fixture repo(untracked/ignored/nested dir を含む)で列挙の integration test +
headless klog 検証 + PM の GUI 目視。

## リスク

- 巨大 repo での列挙コスト(遅延展開が必須)。
- ignore 規則の再実装に走らないこと(必ず libgit2 の status/ignore 判定を使う)。
