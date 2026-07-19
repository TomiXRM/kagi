# ADR-0128: マージ済みブランチの一覧と安全な削除(Branch Cleanup)

- Status: Accepted
- Date: 2026-07-19

## 実装ノート(実装時の確定事項)

- 関数名は `plan_delete_merged_branches` / `execute_delete_merged_branches`
  (preflight は execute 内: 全体の `preflight_check` + ブランチ毎の OID /
  祖先性再検証)。列挙は `collect_branch_cleanup`。
- **テーブル行は `RepoSnapshot.cleanup_rows` として snapshot に載せる** —
  リロードのたびに再分類され、サイドバーの件数バッジ
  (「マージ済みブランチ (N)」、N = merged クラスの行数)が常時出せる。
  ペインは `branch_cleanup_open: bool` の CenterTakeover
  (FileHistory / Ecosystem と同型、ADR-0121 B1 registry)。
- 到達性判定は**ブランチあたり merge_base 1 回**に集約:
  `merge_base(main, tip) == tip` → FullyMerged、
  `merge_base(main, tip) ∈ {mainのmerge commitのparent²}` → MergedThenGrown
  (develop は自分の旧 tip = parent² から分岐を続けるので base がそこに当たる。
  マージ後の main から分岐しただけのブランチは base が main の first-parent
  線上に落ちるので WARN にならない)。初版の「merge ごとに
  graph_descendant_of」は O(ブランチ×マージ) の merge-base 走査になり、
  起動時 snapshot(メインスレッド)で数分固まって GUI が描画されない
  実害が出たため書き換えた。
- klog 契約行: `merged-branches: <n> full, <n> squash?, <n> warn, <n> stale`
  (snapshot 毎)、`branch-cleanup: opened`、
  `plan: branch-cleanup targets=<n> blockers=<n>`、
  `executed: branch-cleanup deleted=<n> failed=<n>`。
- リモート(SSH)リポジトリビューでは cleanup_rows は常に空(非目標)。
- **fetch は `--prune` 付き**(fetch / auto-fetch / pull の全経路)。prune が
  ないと hoster 側で削除済みのブランチが幽霊 `origin/*` ref として永遠に
  残り、テーブルが実在しないリモートブランチを報告する(実使用で発覚)。
  prune は tracking-ref キャッシュの削除のみでローカルブランチ・object
  store には触れず、upstream 消失を `[gone]` として正しく伝播させるので
  squash ヒューリスティックの入力にもなる。

## Context

PR ベースの開発では merge 済みブランチがローカル/リモート双方に溜まり続ける。
掃除は現状 CLI 手作業(`git branch --merged` + `git push --delete`)で、
(1) squash マージだと `--merged` に出ない、(2) merge 済みブランチに後から
コミットが生えているケース(develop 型)を誤って消すと作業が失われる、という
2 つの罠がある。kagi は safety-first Git GUI として「merged 判定の根拠を見せ、
消してよいものだけ消させ、消しても oplog から戻せる」形でこれを提供する
(ユーザー要望 2026-07-19)。

「merged」は一枚岩ではなく 3 クラスある:

| クラス | 判定 | 扱い |
|---|---|---|
| FullyMerged | tip が main の祖先(`graph_descendant_of`) | 一括削除の対象 |
| SquashMergedLikely | tip は祖先でないが upstream が `[gone]`(PR マージ時に GitHub がリモートブランチを削除) | 別バッジ表示。確証がないので**一括からは除外、個別削除のみ** |
| MergedThenGrown | main のある merge commit の `parent^2` が tip の祖先だが、tip 自体は祖先でない(merge 後に N コミット生えた) | **WARN バッジ付きで表示、削除ボタン無効**。削除手段を与えない |

## Decision

1. **分類は純粋関数として `kagi-domain` に置く** —
   `branch_cleanup::classify(...) -> MergedBranchStatus`
   (`FullyMerged { merged_at }` / `SquashMergedLikely` /
   `MergedThenGrown { ahead }` / `NotMerged`)。グラフ到達性・merge commit
   一覧・upstream 状態はデータとして渡す。ユニットテストはここ。
2. **merged_at は main の first-parent walk で取る** — 各 merge commit の
   `parent^2` → merge commit 日時のマップを作る(walk は直近数千コミットで
   打ち切り)。fast-forward マージで merge commit がない場合は tip の
   commit 日時をフォールバック表示。
3. **スコープはローカル + リモート(origin)** — 同名のトラッキングペアは
   1 行にまとめ、local / origin の存在をチップで示す。削除は確認モーダルで
   local / remote を明示した上で両方(または存在する側)を消す。
4. **削除は `crates/kagi-git/src/ops/branch_cleanup.rs` の三点セット** —
   `plan_delete_branches` / `preflight_delete_branches` /
   `execute_delete_branches`。git2 の `Branch::delete` は `-d` と違い
   merged チェックをしない生 API なので、**preflight が唯一の安全弁**:
   plan 時に表示した tip OID と現在の tip の一致 + 祖先性を削除直前に
   再検証する(plan 表示中のコミット追加レースを防ぐ)。リモート削除は
   `cli.rs` 経由の `git push origin --delete <branch>`。実行直前に
   `git ls-remote origin refs/heads/<branch>` で plan 時 OID と照合する
   (`--force-with-lease` は push.rs が「一切使わない」と明文で誓約して
   いるため、削除ガードであっても使わない。ls-remote 照合 → delete の間に
   僅かなレース窓は残るが、直後の verify + oplog の OID 記録で復元可能)。
5. **oplog に tip OID を記録し、undo 可能にする** — undo は
   「その OID でブランチを再作成」(ローカル)/「その OID を push して
   ブランチを再作成」(リモート)。破壊操作を提供しないという製品原則の
   もとで、この undo 可能性が本機能の安全性の核。
6. **UI はブランチリスト上部のエントリ + テーブルビュー** — サイドバーの
   ブランチリスト上部に「マージ済み N 件」ボタン。クリックでペインが
   テーブル表示に切替:列 = ブランチ名 / local・origin チップ /
   マージ日時 / ステータスバッジ / 行ごとのゴミ箱ボタン。ペイン上部に
   「FullyMerged N 件を一括削除」ボタン。WARN(MergedThenGrown)行は
   ゴミ箱無効 + 「マージ後 N コミット」ツールチップ。
7. **除外対象** — 現在の HEAD ブランチとデフォルトブランチ(main)は
   常にリスト外。
10. **ステイル(デッド)ブランチも同じ表で可視化する** — tip の commit
    日時が閾値(v1 は 90 日固定)より古いブランチは、マージ状態と直交する
    `stale` フラグとしてバッジ表示する。**未マージでも stale なら行として
    表示する**(掃除の検討材料として見せるのが目的)。ただし削除可否は
    あくまでマージ分類に従う — stale であること自体は削除を有効にしない。
11. **ブランチ名のコピー** — 各行にコピーボタン(ブランチ名 1 件)、
    ペイン上部に「全ブランチ名をコピー」(改行区切りのプレーンテキスト)。
    AI への調査依頼やターミナル作業への持ち出し用途。
8. **klog 契約行**(headless テスト用、書式は実装時に固定):
   候補列挙時 `[kagi] merged-branches: <full> full, <squash> squash?, <warn> warn`、
   plan / execute / verify は既存 ops の書式に合わせる。
9. i18n: `Msg` に EN + JA を追加。

## 非目標

- patch-id 等価(`git cherry` 相当)による squash マージの確定判定
  (v2 候補。v1 は `[gone]` ヒューリスティックまで)。
- origin 以外のリモート対応。
- WARN ブランチの強確認付き削除(force delete 相当になるため提供しない)。
- 自動掃除(起動時のバックグラウンド削除など)。列挙と手動削除のみ。
- stale 閾値の設定 UI(v1 は 90 日固定。要望が出たら settings.json キーに)。
- stale 未マージブランチの削除(表示のみ。消したければ通常の Git 操作で)。

## Consequences

- squash マージ運用のリポジトリでも `[gone]` 行として候補が見える
  (確証がないため一括には入らない、という保守的な線引き)。
- WARN 行は「なぜ develop が消せないのか」に理由付きで答える UI になる。
- oplog + OID 記録により、削除は全クラスで復元可能な操作になる。
- preflight の OID 再検証により、リスト表示中にブランチが進んだ場合は
  削除が拒否され、リストの再読込を促す。
