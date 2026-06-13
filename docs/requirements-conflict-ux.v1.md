# Requirements: Conflict Resolution UX(Conflict Mode)

- Status: **Accepted(design)**(2026-06-13。サーベイ3本統合済み・ADR-0056〜0061・T-CONFLICT-* 起票済み。実装はユーザー go 待ち)
- 調査: docs/research/conflict-ux-{gui-clients,editors,models}.md(Opus 3レーン、進行中)
- 関連: ADR-0023(操作分類)、ADR-0046(backup思想)、ADR-0048(i18n)、ADR-0052(merge/rebase 方向)

## 0. ゴール(ユーザー原文の要約)

既存 merge UI の模倣ではなく、**「なぜ衝突したか」「どちらを選ぶべきか」「解消後に何が起きるか」を
理解できる** UX。単なる ours/theirs/both 選択 UI は不十分。
merge/rebase/cherry-pick/revert 中の状態を**アプリ全体の一級状態(Conflict Mode)**として扱う。
AI/LLM 補助は later だが差し込める設計に(ローカル LLM は core feature になる予感 — ユーザー)。

## 1. 課題設定(解くべき課題)

ユーザー指定の 13 課題を、設計で答える 5 グループに整理する:

### A. 理解の課題 — 「なぜ衝突したか」が見えない
- conflict の原因(両側で同じ行域を触った2つの**コミット系列**)が UI に出ない。
  → 各 conflict hunk に「この側を最後に触った commit(sha + summary + author + 日時)」を表示する
  **blame-of-sides**。どちらの変更が「何をしようとしていたか」を commit message で比較できるようにする
- AI 生成コード時代は同一箇所に似た変更が入り**意味的衝突**が増える → テキスト一致でも
  「両側が同じ関数を別方向に変えた」ことを警告できる単位(symbol 単位)が将来必要(v1.0+)

### B. 用語の課題 — ours/theirs が操作で意味反転する
- §2 用語設計で解決(ours/theirs を UI から排除)

### C. 進行の課題 — どこまで終わったか・次に何をするか
- 未解決/解決済み/確認待ちの**ファイル単位 + hunk 単位の進捗**を常時表示(N/M)
- 大量 conflict の作業順序: 小さい/機械的に解けるものを先に提案(並べ替え)
- marker 残存の検出(チェックリスト ADR-0043 の conflict marker 検出を解決完了判定に再利用)
- continue / abort / skip を**Conflict Mode の常設バー**に集約(導線迷子を防ぐ)

### D. 安全の課題 — 解決の取り消し・中断・再開
- 解決操作(choose side / edit / accept-both)は**解決バッファ上の操作**で、execute まで repo を
  汚さない(kagi の in-memory 主義の延長)。バッファは draft 同様**自動保存**され中断・再開可能
- undo/redo は解決バッファのファイル単位履歴
- abort は常に安全(開始前 snapshot へ。oplog に before/after)
- 解決結果の **Result preview**(最終ファイル + 結果 diff)を continue 前に必ず見せる

### E. 逃げ道の課題 — GUI で解けない時
- 外部 merge tool 起動(設定で指定)と「terminal で続ける」導線を Conflict Mode から常時提供。
  外部で解決された変化は watcher で取り込み、Mode の進捗に反映

## 2. 用語設計(確定)

**ours / theirs は UI に出さない。**全操作で「役割 + 実名」の2行ラベルに統一する:

| 操作 | 左側(index stage 2 = ours) | 右側(stage 3 = theirs) |
|------|------------------------------|--------------------------|
| merge | **Current branch** `main` | **Merging in** `feature/x` |
| rebase | **New base** `main`(注: git 内部では ours が逆転するが UI は役割で固定) | **Your commit being replayed** `abc123 "msg"` |
| cherry-pick | **Current branch** `main` | **Commit being applied** `abc123 "msg"` |
| revert | **Current branch** `main` | **Changes being undone**(revert of `abc123`) |

- 共通: **Base**(共通祖先)と **Result**(編集可能な解決結果)。3-way + Result の4役割
- 各 hunk のボタン文言も役割で: 「Keep current (`main`)」「Take incoming (`feature/x`)」
  「Keep both (current first)」等。**操作種別ごとにヘッダで「いま何の途中か」を常時表示**
  (例: 「Rebasing `feature/x` onto `main` — commit 2/5」)
- i18n: 役割語は Msg(ADR-0048)で en/ja、branch/commit 名は実名のまま

## 3. Conflict Mode(一級状態)の設計骨子

```
enum RepoMode { Normal, Conflict(ConflictSession) }
struct ConflictSession {
  op: Merge|Rebase{step,total}|CherryPick{..}|Revert{..},   // git_repository_state + state files
  files: Vec<ConflictFile>,        // path, kind(content/rename-delete/modify-delete/binary), status
  resolution: ResolutionBuffer,    // ファイルごとの Result 草稿 + undo 履歴(自動保存、~/.kagi/)
}
```
- 検出: `git_repository_state()` + index conflict entries(stage 1/2/3)。外部(CLI)で発生した
  conflict も起動時/ watcher で検出して Mode に入る(**アプリ全体が Mode を知る**: header バナー、
  sidebar 進捗、graph の対象 commit 強調、危険操作の disabled — BCM の conflict_mode 入力と接続)
- continue = 解決バッファを index/WT へ書き出し → marker 検査 → stage → 各操作の継続
  (merge commit / rebase next / sequencer continue)。abort = 開始前状態へ(常時可能)。
  すべて plan→confirm→…→oplog パイプライン上で
- LLM 差し込み点(later): (a) hunk の「両側の意図」要約 (b) 解決案の提案(Result 草稿に挿入、
  ADR-0044 と同じ opt-in / localhost / staged 同意モデル)— **解決の自動適用はしない**

## 4. フェーズ分け(案 — サーベイ統合後に確定)

| フェーズ | 内容 |
|----------|------|
| **MVP** | Conflict Mode 検出 + 常設バナー(continue/abort 導線)/ conflict file list + 進捗 / 用語設計の適用 / ファイル単位 choose(current/incoming/both)+ marker 検査 / Result preview / abort 安全保証 / 解決バッファ自動保存 |
| **v0.2** | hunk 単位 choose + Result 手編集(3-way + Result view)/ blame-of-sides(原因 commit 表示)/ undo・redo / rename・delete / binary の明示 UI / 外部 tool・terminal 連携 |
| **v1.0** | 作業順序の提案 / rebase 多 step 進捗 UX / rerere 相当の再利用 / 意味的衝突の警告(symbol 単位の入口) |
| **later** | LLM 意図要約・解決案 / symbol 単位解決 / 自動解決ポリシー |

## 4.5 サーベイ統合(確定した取り込み/不採用)

**取り込む**(出典: conflict-ux-{gui-clients,editors,models}.md):
- 実ブランチ名ラベル(Fork が最良。SourceTree の Mine/Theirs 反転バグ SRCTREE-1670 が反例)→ §2 確定
- KDiff3 の安全機構: 全解決まで continue 無効 + 未解決数 + prev/next 未解決ナビ
- JetBrains: non-conflicting 一括適用 / Accept 系キーボードショートカット
- both 採用時の順序明示(Combination current-first / incoming-first)
- 出所可視化(手編集箇所・行ごとの採用元、BC/KDiff3 流)
- zdiff3 marker style で Base 文脈を表示(git2 MergeFileOptions::style_zdiff3)
- ours/theirs の rebase 反転は `Repository::state()` を見て文脈名へ翻訳(モデル調査 §4)
- jj から借りる: **部分解決を失わせない**(解決バッファ自動保存 + abort 時も oplog へ退避)
- binary / rename-delete / modify-delete の専用 UI(全 GUI が弱い = 差別化点)

**不採用**:
- jj の first-class conflict(commit へ conflict 埋め込み)と fearless rebase — git 標準互換を壊す(Reject)
- VSCode 型の view 強制切替 — 退路(inline marker 直接編集)を必ず残す
- GitKraken 型の内蔵エディタ有料化文脈の UI 分断 / GitHub Desktop 型の外部任せ
- AI 自動解決(later でも「提案まで」。自動適用はしない)

**実装基盤(確定)**: git2 0.21 の `Index::conflicts()` / `Repository::state()` / `cleanup_state()` /
`merge_commits`・`merge_trees`(無傷 dry-run)/ `MergeFileResult` — 3-way 解決 UI の基盤は揃っている。

## 5. 次のステップ

1. 調査3本を merge → 各ツールの「取り込む/取り込まない」を本書 §1/§4 に反映
2. ADR 起票(予定): Conflict Mode state machine / 解決バッファと undo / 用語(§2 を ADR 化)/
   3-way+Result view アーキテクチャ / 外部 tool 連携 / LLM 差し込み点
3. T-CONFLICT-* チケット分割(実装はユーザー go 後)
