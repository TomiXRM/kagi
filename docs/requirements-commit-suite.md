# 要件定義: Commit 便利機能スイート

- Date: 2026-06-13
- 発端: ユーザー要件(原文は本ファイル末尾に転記)
- 関連 ADR: 既存 0011 / 0022 / 0023 / 0031 / 0037、新規 0039〜0045
- 実装担当: PM + Sonnet subagent(worktree lane 方式、PM が merge・検証)

## 目的(4本柱)

1. **間違った変更をコミットしない** — Commit Preview + Checklist(staged の事前可視化、危険パターンの block/warn)
2. **コミット粒度の整理** — Split Commit 支援(file 単位、既存 stage/unstage UX の整理)/ New Branch への commit
3. **メッセージ作成の負担軽減** — Draft Autosave / Message Template / Smart Commit Message(ローカル LLM)
4. **amend / undo / fixup の安全な履歴編集** — 既存 undo の追補 + amend 設計 + fixup の作成のみ(autosquash later)

**不変条件(既存の安全方針を継承)**:
- repository を変更する操作は必ず **plan → confirm → preflight → execute → verify → oplog** に乗る(ADR-0022)
- history-rewriting(amend 含む)は ADR-0023 のカテゴリ表に従う。**force push は絶対に提案/実装しない**
- destructive plan は実行前に現在 HEAD の SHA を oplog に記録(recovery 起点)
- **ネットワーク送信は最小**(ADR-0037)。Smart Commit はデフォルト localhost のみ。外部 API はユーザーが明示設定した場合に限る

---

## ギャップ分析(2026-06-13 実装時点)

| 機能 | 既に実装済み | 今回やる(MVP) | later / v0.2 |
|------|--------------|----------------|--------------|
| Commit Panel 基盤 | stage/unstage(file 単位 + 一斉)/ commit message Input(IME・gpui-component)/ `plan_commit`+`execute_commit` / conflict ファイル表示 | — | — |
| Commit Preview | plan modal に staged 反映あり / staged diff は Commit Panel の diff viewer 経由 | **staged files count / changed files summary(A/M/D 別)/ staged diff preview / target branch / author を Commit Panel に常設表示**(plan modal にも要約) | preview の hunk 単位ハイライト |
| Commit Checklist | `plan_commit` が「staged なし」「message 空」「conflict 状態」を blocker、leftover を warning に判定済み | **conflict marker 検出 / unresolved conflict file 不可 / secret・.env 警告 / large binary 警告 を追加**(ADR-0043 のルール表) | カスタムルール設定 |
| Draft Autosave | なし(commit message は session 内のみ) | **branch ごと保存 / 再起動復元 / 成功時 clear**(`~/.kagi/drafts/`、手書き JSON、debounce)(ADR-0042) | per-worktree draft |
| Message Template | なし(plain text Input のみ) | **type/scope/summary/body/test/risk テンプレ / plain⇄template 切替**(ADR なし、Commit Panel UI のみで実現) | テンプレのユーザーカスタム |
| Amend | なし(`undo_commit` で擬似的に可能だが UX なし) | **message only / staged / 両方 / SHA 変化表示 / pushed は強警告**(ADR-0040、2段階確認)。backend = `plan_amend` / `execute_amend` | reword(古い commit)は履歴書き換えのため対象外 |
| Undo Last Commit | **実装済み**(T-HT-009 / ADR-0011: ref 付け替えのみ soft 相当 / pushed blocker / oplog に元 sha) | **要件との突合のみ。差分なし**(ADR-0041 で確認)。reset hard 禁止の再確認 | push 済み commit の取り消し(= revert、別機能) |
| Smart Commit Message | なし。ureq 3 は依存にあり(avatar で実績) | **staged diff から生成 / Conventional Commits / 日英 / ローカル LLM(ollama)第一バックエンド / rule-based fallback**(ADR-0044) | external API バックエンド(設定で opt-in)/ ストリーミング表示 |
| Split Commit | 実質「既存 stage/unstage で複数回 commit」 | **file 単位の UX 整理(チェックで部分 commit → 残りは未 staged のまま)+ ガイド表示** | hunk 単位 split |
| Commit to New Branch | branch 作成は `plan_create_branch` あり / commit は `execute_commit` あり | **commit 直前に「現在 branch ではなく新 branch に commit」フロー(branch 作成 → checkout → commit を 1 plan に)**(ADR-0040 と独立、既存 plan の合成) | detached からの commit |
| Fixup / Squash | なし | **fixup!/squash! prefix の commit を作成するだけ**(対象 commit 選択 → message 自動生成、履歴書き換えなし)(ADR-0045) | autosquash 実行(rebase -i 相当) |

### 既存実装で「充足」のものの根拠

- **Undo Last Commit(要件 MVP)**: T-HT-009 / ADR-0011 が「soft reset 相当(ref 付け替えのみ)/ index・WT 不変 / pushed blocker / merge commit blocker / detached blocker / oplog に元 sha」を満たす。要件の「reset hard 禁止」「pushed 対象外」「oplog に before/after HEAD」を全て充足。→ チケット **T-COMMIT-013/014 は done 相当**(ADR-0041 は突合 ADR)
- **Commit Checklist の一部**: `plan_commit`(staging.rs)が「staged なし不可 / message 空不可 / conflict 状態不可 / leftover 警告」を実装済み。→ **追加分は conflict marker / secret / large binary の 3 ルールのみ**

---

## MVP / v0.2 スコープ整理

### MVP(本スイートの主目標 = 安全な commit 体験)

- Commit Preview(staged 可視化)
- Commit Checklist(危険パターンの block / warn)— ADR-0039 / 0043
- Draft Autosave — ADR-0042
- Message Template(plain⇄template)
- Amend(message only / staged / both、pushed 強警告、SHA 変化)— ADR-0040
- Undo Last Commit — 既存で充足(ADR-0041 で突合)

### v0.2(粒度・自動化)

- Smart Commit Message(ローカル LLM + fallback)— ADR-0044
- Split Commit 支援(file 単位)
- Commit to New Branch
- Fixup / Squash 作成のみ — ADR-0045

### 明示的に MVP 外 / later

- hunk 単位 split / staging
- autosquash 実行(rebase -i 相当)
- 古い commit の reword / 任意 commit の編集(= 履歴書き換え。設計しない)
- external LLM API(設定で opt-in した場合のみ。デフォルト無効)

---

## 完了条件(スイート全体)

- [ ] requirements / ADR 0039〜0045 / ticket T-COMMIT-001〜018 / INDEX 追記 が worktree branch に commit 済み
- [ ] 0040(amend の pushed 扱い)と 0044(既定バックエンド)が **Proposed** で、ユーザー判断点が明記されている
- [ ] 各 ADR の実装方式が in-memory 主義・ref-order 規則・serde 禁止(手書き JSON)・ネットワーク最小に整合
- [ ] 既存実装で充足するチケットは done 相当 + 根拠つき

---

## 実装 lane 分割案(W14-x)

PM が main 側で UI 配線、subagent が backend を worktree で並行。lane 間のファイル衝突を最小化する分割:

| lane | 内容 | 主な触る backend | 依存 |
|------|------|------------------|------|
| **W14-CHECK** | Commit Checklist ルール(conflict marker / secret / large binary)を `plan_commit` に統合 | `src/git/staging.rs`(plan_commit 拡張)+ 新 `src/git/checklist.rs` | ADR-0039 / 0043 |
| **W14-PREVIEW** | Commit Preview(count / summary / staged diff / target branch / author)を Commit Panel に常設 | `src/ui/commit_panel.rs`(UI のみ。backend は既存 staging API) | — |
| **W14-DRAFT** | Draft Autosave backend(branch ごと保存・復元・clear、手書き JSON) | 新 `src/git/drafts.rs` + UI 配線(debounce は `schedule_modal_replan` を参考) | ADR-0042 |
| **W14-TEMPLATE** | Message Template(plain⇄template、構造化フィールド組み立て) | `src/ui/commit_panel.rs`(UI + 純関数の組み立て) | W14-DRAFT(保存形式の整合) |
| **W14-AMEND** | Amend backend(`plan_amend` / `execute_amend`、3 モード、pushed 判定、SHA 変化) | `src/git/ops.rs`(plan_amend)+ `staging.rs`(execute) | ADR-0040 |
| **W14-NEWBRANCH** | Commit to New Branch(branch 作成 + checkout + commit の合成 plan) | `src/git/ops.rs` / `staging.rs`(既存 plan 合成) | — |
| **W14-SMART** | Smart Commit Message(LLM backend enum dispatch + ollama + rule-based fallback) | 新 `src/git/message_gen.rs`(ureq 再利用) | ADR-0044 |
| **W14-SPLIT** | Split Commit 支援(file 単位 UX、ガイド) | `src/ui/commit_panel.rs`(UI、既存 stage/unstage 流用) | W14-PREVIEW |
| **W14-FIXUP** | Fixup/Squash 作成(prefix commit のみ) | `src/git/ops.rs` / `staging.rs`(message prefix のみ) | ADR-0045 |

lane 衝突注意:
- `staging.rs` の `plan_commit` は W14-CHECK と W14-AMEND が触る → checklist は別 module(`checklist.rs`)に切り出し、`plan_commit` からは関数呼び出しのみにして衝突面を狭める
- `commit_panel.rs` は W14-PREVIEW / W14-TEMPLATE / W14-SPLIT が触る → PM が main 側で統合する前提。subagent は backend 優先

---

## ユーザー要件原文(転記)

> **(MVP)** Commit Preview: staged files count / changed files summary / staged diff preview / target branch / author。Commit Checklist: staged なし不可 / message 空不可 / conflict marker 警告 / unresolved conflict file 不可 / secret・.env 警告 / large binary 警告。Draft Autosave: branch ごと / 再起動復元 / 成功時 clear。Message Template: type/scope/summary/body/test/risk、plain⇄template 切替。Amend: message only / staged / 両方、pushed は強警告、SHA 変化表示。Undo Last Commit: soft reset 相当 / pushed 対象外 / reset hard 禁止 / oplog に before/after HEAD。
>
> **(v0.2)** Smart Commit Message: staged diff から生成(unstaged 含めない)/ Conventional Commits / 日英。Split Commit 支援(file 単位、hunk later)。Commit to New Branch。Fixup/Squash(autosquash later)。
>
> **(備考)** smart commit はたとえば私の PC には ollama で gemma が入っている。そういうのをバックエンドとして使う設計もあり。設計が大事。
