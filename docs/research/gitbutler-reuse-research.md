# GitButler 流用調査

- 調査日: 2026-06-12 / 調査者: research subagent
- 対象: `gitbutlerapp/gitbutler` shallow clone (`/tmp/kagi-research/gitbutler`)
- ライセンス: **FSL-1.1-MIT(Functional Source License v1.1 / MIT Future License)**(原文確認済: `/tmp/kagi-research/gitbutler/LICENSE.md`)
- 関連: ADR-0033(gitbutler concepts adoption)、ADR-0031(流用ポリシー)

## ライセンスゲート(最重要)

FSL-1.1-MIT の核心(原文 §Permitted Purpose / §Competing Use):

- 許諾されるのは **Permitted Purpose のみ**。**Competing Use** は禁止。Competing Use の定義:
  「商用製品・サービスとして他者に提供し、(1) 本ソフトの代替、(2) 本ソフトを使った我々の他製品の代替、または **(3) 同一・実質的に類似する機能** を提供するもの」。
- kagi は **Git GUI クライアント**であり、GitButler も Git GUI。(3)「実質的に類似する機能」に該当する蓋然性が高く、GitButler のソースを kagi に取り込んで配布する行為は **Competing Use = ライセンス違反**になり得る。
- §Grant of Future License: 各バージョンは公開から **2 年後に MIT へ自動転換**。つまり「2 年以上前のコミットのコード」は MIT として利用可能だが、commit 単位で日付確認が必要で運用が煩雑。
- **結論(ゲート)**: GitButler は **コード流用不可(concept adoption のみ)**。MIT 転換済みの古いコードを使う場合のみ、ADR-0031 の手順で commit 日付を原文確認した上で例外検討。VirtualBranch を MVP に入れない初期仮説を強化。

## クレート構成の所見

`/tmp/kagi-research/gitbutler/crates/` 配下に `but-*`(新世代・ライブラリ寄り)と `gitbutler-*`(レガシー)が混在。多くが **gix 主体 + git2 をレガシー fallback**、**sqlite/DB(but-db)** と **Tauri デスクトップアプリ**に結合。pure-logic crate と app 結合 crate の見極めが必要。

## 観点ごとの findings

### 1. Virtual branch / parallel branch

- crate: `but-workspace`, `gitbutler-workspace`, `but-graph`
- ファイル: `crates/but-workspace/src/lib.rs`、`crates/but-graph/src/projection/workspace/mod.rs`
- モデル: 複数 stack(branch)を**単一 worktree に同時適用**。統合は **workspace commit(octopus merge)** で全 stack tip を 1 tree にまとめ、`refs/heads/gitbutler/workspace` がそれを指す。`Workspace { stacks: Vec<Stack> }`。
- 評価: kagi の安全パイプライン(plan→confirm→…)と思想が真逆寄り。virtual branch は HEAD を GitButler 管理下に置き worktree を書き換える侵襲的モデル。**MVP に入れない**(初期仮説どおり)。

### 2. Stacked branches

- crate: `gitbutler-stack`, `but-graph`
- ファイル: `crates/gitbutler-stack/src/lib.rs`、`crates/but-graph/src/projection/stack.rs`
- 型: `Stack { id, segments: Vec<StackSegment> }`、`StackSegment {{ ref_info, commits, base, base_segment_id, remote_tracking_ref_name, ... }}`。各 segment の base が次 segment の tip = **依存ブランチの連鎖**。first-parent walk で線形化(`StackCommit`)。
- 評価: 「依存ブランチを stack として束ね、各層を独立 branch として扱う」**概念は kagi の change lane 思想(初期仮説の Reimplement 候補)と整合**。データモデル(segment + base pointer)は参考価値が高い。ただし but-graph は gix + petgraph に結合。

### 3. Hunk assignment / hunk dependency

- crate: `but-hunk-assignment`, `but-hunk-dependency`
- ファイル: `crates/but-hunk-assignment/src/lib.rs`、`crates/but-hunk-dependency/src/lib.rs`
- assignment: `HunkAssignment {{ id, hunk_header, path, stack_id, line_nums_added/removed }}`。worktree diff をスキャン → 永続 assignment と fuzzy reconcile(重複時は行数最大を採用)。
- dependency: worktree hunk が「どの commit と衝突するか」を BranchTip→Base で適用しながら追跡。`AmendableCommit`(綺麗に当たる)/ `IntroducingCommit`(初衝突)を返し hunk を自動 lock。
- 依存: `but-hunk-dependency/Cargo.toml` は `but-core` `but-ctx` `but-graph` `gix` に依存(**but-ctx 経由で gix・workspace 全体を引き込む**)。「pure logic」と説明されるが実際は GitButler context に結合。
- 評価: 「hunk をどのコミット/レーンに割り当てるか」の**アルゴリズム概念**は将来 partial staging の高度化に有用。ただし FSL + gix + but-ctx 結合でコード流用は不可。

### 4. Undo(oplog / snapshot)

- crate: `but-oplog`(新), `gitbutler-oplog`(レガシー, git2 使用)
- ファイル: `crates/but-oplog/src/lib.rs`
- モデル: **snapshot = Git tree**(HEAD 位置・全 ref 位置・GitButler メタを直列化、worktree 変更や untracked は含まない)。oplog = snapshot tree を指す commit 連鎖、メタは `operations-log.toml`。`UnmaterializedOplogSnapshot` で「操作成功時にだけ commit」= all-or-nothing。
- 評価: **kagi の安全パイプライン(verify 後に確定)と非常に相性が良い思想**。「操作前に状態を snapshot、失敗なら oplog に残さない」は kagi の oplog(`src/git/oplog.rs`)を堅牢化する設計言語。content を Git object store に置く点は kagi の JSONL とは別実装だが、**atomicity 思想は concept adopt 価値が高い**。

### 5. Git backend 抽象

- gix 主体(`but-core` `but-workspace` `but-graph` `gitbutler-git` `but-ctx`、features に dirwalk/credentials/merge/status 等)、git2 はレガシー fallback(`gitbutler-oplog` `gitbutler-workspace` 等)。
- 抽象は `but-core` の `RepositoryExt` / `ObjectStorageExt`、context は `but-ctx::Context`(repo+workspace+db、permission `RepoShared`/`RepoExclusive`)。
- 評価: gix 前提。kagi の git2 方針と非互換。backend 層は流用不可。

### 6. Worktree

- crate: `but-worktrees`(`crates/but-worktrees/src/lib.rs`)
- 型: `WorktreeId`(UUID)、`WorktreeMeta {{ id, created_from_ref, base }}`、`Worktree {{ id, path, created_from_ref, base }}`。モジュール: new/destroy/list/integrate/db/git。
- 評価: kagi の WORKTREES(ADR-0014 で v0.2、ADR-0025 worktree creation UX)に概念参考。worktree id を stack id と直交させる設計は素直。ただし DB 永続(but-db)結合。

### 7. Graph / commit-graph 分析

- crate: `but-graph`(`crates/but-graph/src/lib.rs`, `segment.rs`)
- モデル: petgraph 上の **Segment**(ref 境界 or 分岐で区切られた commit 束)。3 段処理: traversal(commit graph → segmented graph)→ reconciliation(workspace メタ統合)→ projection(stack ビュー化)。merge 保持・entrypoint(HEAD focus)・soft/hard limit。
- 評価: 「segment 単位の graph 抽象」は kagi の自前 lane layout(ADR-0003)より高機能だが、用途が virtual branch projection 向け。kagi の commit graph 表示には over-engineered。**Study only**。

### 8. Agent workflow 支援

- crate: `but-action`(worktree 変更 → 自動コミット)、`but-tools`(OpenAI function 定義)、`but-rules`(`WorkspaceRule {{ trigger, filters, action }}` 宣言的自動化)、`but-llm`(OpenAI/Anthropic/Ollama/LM Studio/OpenRouter 統合)。
- 評価: kagi の MVP スコープ外。AI 連携は将来検討。`but-rules` の「ファイル変更 → アクション」宣言モデルは将来 automation の参考程度。**Study only**。

## 候補テーブル

| 候補 | 分類 | 理由 | コスト | リスク |
|---|---|---|---|---|
| Oplog snapshot atomicity(`UnmaterializedOplogSnapshot` 思想) | **Reimplement** | 「操作前 snapshot・成功時のみ確定」が kagi 安全パイプラインと完全整合。kagi の JSONL oplog に概念を自前実装で取り込む | 中 | 低 |
| Stacked branch データモデル(segment + base pointer) | **Study only**(将来 Reimplement) | change lane 思想と整合。だが gix/petgraph 結合・FSL のためコードは不可。概念を kagi 型で将来再設計 | 大 | 中 |
| Hunk assignment / dependency アルゴリズム | **Study only** | partial staging 高度化の将来参考。but-ctx 結合・FSL でコード不可 | 大 | 中 |
| Worktree モデル(id を stack と直交) | **Study only** | ADR-0025 worktree UX の概念補強。DB 結合のためコード不可 | 小 | 低 |
| but-graph segment 抽象 | **Study only** | kagi の lane layout には過剰。virtual branch 用途 | 中 | 低 |
| Virtual / parallel branch(workspace commit) | **Reject(MVP)** | HEAD 侵襲・安全パイプラインと思想衝突・FSL。**MVP に入れない判断は禁止事項どおり堅持** | 特大 | 高 |
| Agent workflow(but-action/rules/llm) | **Study only** | MVP スコープ外。将来 automation の参考 | 小 | 低 |

## kagi への具体的提案

1. **oplog の atomicity 強化(最優先 concept)**: kagi の `src/git/oplog.rs` に「操作前 snapshot を作り、verify 成功時にのみ oplog エントリを確定、失敗なら破棄」という `UnmaterializedOplogSnapshot` 相当の二段階を**自前再実装**で導入する価値あり。GitButler のコードはコピーせず、概念のみ。安全パイプライン(plan→…→verify)に自然に乗る。
2. **stacked branch は思想だけ先に固める**: change lane / stacked branch を将来導入する際の **データモデル参考**として but-graph の segment+base 構造を Study に記録。実装は kagi の git2 型で全面再設計。MVP には入れない。
3. **virtual branch は明確に Reject(MVP)**: 禁止事項どおり。HEAD 管理・worktree 書き換えは kagi の「destructive operation 禁止・無傷 in-memory merge」方針と衝突。
4. **コード流用は一切しない**: FSL-1.1-MIT の Competing Use により Git GUI である kagi への取り込みは違反リスク。2 年経過 MIT 転換コードの例外利用も、煩雑さに見合わないため原則行わない(ADR-0031 のゲートで遮断)。
5. **hunk assignment / worktree / agent は Study に留置**: 将来機能の設計言語として参照。

## 確認できなかった事項

- 各 `but-*` crate の commit 日付ごとの MIT 転換状況(2 年ルール)は個別 commit 確認が必要で未実施。原則「概念のみ」のため実害なし。
- but-oplog の snapshot tree のバイナリ詳細(kagi は JSONL 別実装のため不要)。
