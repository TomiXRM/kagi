# Requirements: Conflict Resolution UX(Conflict Mode)— v2

- Status: **Accepted(design)**(2026-06-13。v1 を GitKraken UX 分解 + 詳細 MVP 仕様で刷新。
  旧版は requirements-conflict-ux.v1.md)
- 調査: docs/research/conflict-ux-{gui-clients,editors,models}.md
- 設計 ADR: 0056(Mode state machine)/ 0057(resolution buffer & undo)/ 0058(用語)/
  0059(view arch)/ 0060(外部ツール)/ 0061(LLM)/ **0062(session model)/ 0063(dashboard)/
  0064(editor layout)/ 0065(file type)/ 0066(save & marker safety)/ 0067(continue/abort/skip safety)**
- 実装状況: W26(backend: conflicts.rs/resolution.rs)+ W30(MVP UI: banner/file list/file 単位 choose/
  Result preview)+ W31(予測 conflict merge → Conflict Mode 遷移)が **完了**。本書はそこから
  GitKraken 同等の Dashboard + 専用 Conflict Editor(hunk 単位)+ 安全強化までの完全仕様。

## 0. ゴール

GitKraken の見た目をコピーするのではなく、その良い UX を分解して **より安全で分かりやすい
Conflict Mode** として再設計する。核は「なぜ衝突したか/どちらを選ぶか/解消後に何が起きるか」を
理解できること、そして**安全(repo を壊さない・常に abort で戻せる・marker 残存は実行不可)**。

## 1. GitKraken UX の分解(取り込む/変える/捨てる)

| # | GitKraken の挙動 | kagi の方針 |
|---|------------------|-------------|
| 1 | graph 上で merge 失敗を明示 | **取り込む**: graph で対象 commit/HEAD を強調 + Mode 表示 |
| 2 | 上部に警告 banner | **取り込む**: Conflict Banner(警告 icon + operation summary + count + Continue/Abort/Open Panel) |
| 3 | 「何を何に」表示(`Merge branch 'test3' into text`) | **取り込む(必須)**: 方向を文言で(ADR-0058) |
| 4 | Right Panel が Conflict Panel に切替 | **取り込む**: Right Panel = Conflict Dashboard(ADR-0063) |
| 5 | `Merge conflicts detected` で Mode 明示 | **取り込む** |
| 6 | current/incoming を badge 表示 | **取り込む**(役割+実名 badge) |
| 7 | Conflicted / Resolved を分離 | **取り込む** |
| 8 | conflict file list | **取り込む** |
| 9 | Path / Tree 切替 | **取り込む**(MVP は Path、Tree は v0.2) |
| 10 | 下部に commit message → merge commit まで同流れ | **取り込む**: continue で commit message 経由 |
| 11 | `Abort Merge` 常時可視 | **取り込む(必須)** |
| 12 | A/B 左右 + 下 Output | **取り込む**: Conflict Editor(ADR-0064) |
| 13 | 各 side に checkbox | **変える**: checkbox は意味曖昧 → **文言が明確なボタン**(Accept current/incoming/both 順) |
| 14 | 選択が Output に即反映 | **取り込む(必須)**: Result Preview 即時更新(ADR-0064) |
| 15 | `conflict 1 of 1` | **取り込む**: hunk ナビ |
| 16 | Auto-resolve with AI / external tool / Save / Close | AI は **MVP 外・差し込み口のみ**(ADR-0061)/ 外部ツール・Save は取り込む |
| 17 | Reset で選択状態を戻す | **取り込む**: Reset hunk / Reset file |
| 18 | Save で結果保存 | **取り込む**: Save resolution + marker 安全チェック(ADR-0066) |
| 19 | 右上に外部ツール導線 | **取り込む**(ADR-0060) |
| — | **`Mark All Resolved`** | **そのまま採用しない(危険)**: 代替は §2.3 |

## 2. 設計思想(ユーザー指定)

### 2.1 Conflict Mode はアプリ全体の Mode(ADR-0056)
merge/rebase/cherry-pick/revert 中に conflict したら **Header / Commit Graph / Right Panel /
Bottom Panel / Status Bar / Commit Panel / Operation Log** を Conflict Mode 用に変化させる。

### 2.2 「何を何に入れて失敗したか」を必ず表示(ADR-0058)
`Merging test3 into text` / `Rebasing feature/foo onto master` / `Cherry-picking abc123 onto master` /
`Reverting commit abc123 on master`。UI は ours/theirs を前面に出さず **Current branch / Incoming
branch / Commit being applied / Base / Result** で表示(内部は git の ours/theirs を扱う)。

### 2.3 Conflict Dashboard を Right Panel に(ADR-0063)
Merge conflicts detected / operation summary / current・incoming badge / conflicted・resolved count /
Path・Tree toggle / Conflicted Files / Resolved Files / conflict type badge / Mark resolved・Reset /
Continue・Abort・Skip。
**`Mark All Resolved` は危険なので不採用**。代替: `Mark selected file resolved` /
`Mark all clean files resolved`(= marker 無し & index resolved のものだけ)/ `Mark all resolved` は
Advanced 扱いで marker 検出・unmerged index 確認後でなければ不許可。

### 2.4 Conflict Editor を専用画面に(ADR-0064)
A(current)/ B(incoming)左右 + 下 Result/Output。hunk ごとに Accept current / Accept incoming /
Accept both(current→incoming / incoming→current)/ Edit result / Reset this hunk。
**ボタン文言で意味を明示**(checkbox にしない)。Top Toolbar: file path / `conflict n of m` /
prev / next / Open external tool / Reset / Save。

### 2.5 Result/Output を一級表示(ADR-0064)
A/B 選択で Result Preview 即更新 / 由来 side 表示 / 未解決 hunk 明示 / marker 残存は保存不可 or 強警告 /
Save 前に result diff 確認 / Save 後 file を resolved candidate に。

### 2.6 Continue / Abort / Skip / Mark resolved / Save は Plan 経由(ADR-0067)
直接実行しない(GitOperationPlan / ConflictOperationPlan を生成)。Continue 前チェック:
unresolved==0 / marker 無し / index resolved / binary 残無し / required file 削除無し /
commit message 非空 / checklist blocker 無し。

### 2.7 Operation Log と Resolution Log を統合
session id / operation kind / current・incoming / conflicted files / selected file /
hunk ごとの resolution action / save 時刻 / continue・abort・skip / before-after hash / marker check。

## 3. kagi 独自の改善

### 3.1 Conflict Resolution Session(ADR-0062)
conflict 発生〜continue/abort までを 1 session として扱う(id/operation/branches/files/counts/
can_continue・abort・skip)。中断・再開で resolution buffer(ADR-0057)を失わない。

### 3.2 Conflict File Type を明示(ADR-0065)
both modified / added by both / deleted by current / deleted by incoming / modified-delete /
rename-delete / rename-rename / binary / submodule。**MVP は both modified 最優先**、その他は
専用選択 UI or 外部ツールへ逃がす。

### 3.3 ours/theirs を UI から隠す(ADR-0058)
Current branch / Incoming branch / Commit being applied / Result を優先。内部 git 用語は tooltip で補足。

### 3.4 Save/Continue 前の安全チェック(ADR-0066)
marker 残存(`<<<<<<<` `=======` `>>>>>>>`)/ unresolved index / empty result / binary 未解決 /
deleted file 判断未了 を検査。**marker 残存は blocker**。

### 3.5 外部ツールへの逃げ道(ADR-0060)
Open in external merge tool / Open terminal at repo root / Copy conflict path / Copy git command。

### 3.6 AI 補助は MVP 外・差し込み口のみ(ADR-0061)
Explain conflict / Suggest resolution / Generate result / Risk summary / Test suggestion を将来差込。
制約: AI が勝手に Save/Continue しない・必ず preview・ユーザー承認必須・送信内容は ADR 必須・
local/remote LLM を区別。

## 4. MVP 要件

- **検出**: merge/rebase/cherry-pick/revert 中、conflicted files、unmerged index、marker、
  can_continue/abort/skip(W26 で大半実装済み)
- **Banner**(画面上部): 警告 icon + operation summary + conflict count + current + incoming +
  Continue / Abort / Open Conflict Panel。例 `Merge conflict detected: merging test3 into text — 1 file conflicted`
- **Right Panel = Conflict Dashboard**: summary / current・incoming badge / Path・Tree / Conflicted /
  Resolved / selected file preview / Abort / Continue / external tool(W30 はファイル単位までは実装、
  Dashboard 化と Resolved セクションは本フェーズ)
- **Conflict Editor**: A/B 左右 + Result preview / hunk ナビ / accept current・incoming・both /
  reset hunk / save / external tool(**hunk 単位は本フェーズの新規**。W30 はファイル単位 choose のみ)
- **Continue/Abort**: Continue は未解決/marker 残で disabled、Abort は確認あり(保存済み resolution が
  消える可能性を表示)、Skip は rebase/cherry-pick のみ(merge は非表示)

## 5. v0.2 以降

Tree view / batch resolve / conflict minimap / syntax highlight / inline editor / hunk ごと undo /
keyboard shortcuts / semantic section 検出 / function・symbol 単位ナビ / resolved result diff /
external merge tool config / AI explain・propose / test command 統合。

## 6. チケット体系(本書で再編)

旧 T-CONFLICT-001..033(v1 設計)は本 Phase 体系に置換。W26/W30/W31 で満たされた項目は done 印。
Phase 1 Conflict State(001-005)/ Phase 2 Dashboard(010-015)/ Phase 3 Editor(020-025)/
Phase 4 Resolution Actions(030-035)/ Phase 5 Continue/Abort/Skip(040-044)/
Phase 6 Escape Hatch(050-052)。詳細は docs/tickets/T-CONFLICT-*.md と INDEX。

## 7. 完了条件(ユーザー指定)

conflict 発生で全体が Conflict Mode へ / 何を何に merge/rebase/cherry-pick しているか分かる /
file 一覧が見える / unresolved・resolved count / file 選択で Conflict Editor / A・B と Result preview /
Accept で Result 更新 / Save resolution / marker 残存で Continue 不可 / Continue・Abort・Skip は Plan 経由 /
Operation Log に解決操作 / 外部ツールへ逃げられる / AI は MVP 外だが拡張口がある。
