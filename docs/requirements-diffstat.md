# Requirements: Per-file Diffstat / Diffstat Mini Bar / Changed File Summary Bar

- Status: Accepted(2026-06-13、ユーザー発案・原文準拠)
- 関連: docs/research/rgitui.md(T-DST-1/2 の原案)、ADR-0039(commit preview)、W7-INSPECTOR2

## 目的

ファイル一覧を見たときに、各ファイルが**どのくらい変更されたのか、追加が多いのか削除が多いのか**を
一目で分かるようにする。

例: `M +4 -3` のように file status / additions / deletions を表示し、右端に緑/赤の小さい bar を出す。
緑=追加行、赤=削除行。変更量が大きいファイルほど bar が目立つ。追加だけなら緑中心、削除だけなら赤中心。

## UI 要件

Changed Files list / Commit Inspector / Commit Panel の各ファイル行に、右側の diffstat 表示を追加する。

各行に表示する情報:

- file path
- file status: A / M / D / R / C / U
- additions count
- deletions count
- diffstat mini bar

表示例:

```text
M  crates/kagi_git/src/project/local_ops.rs   +8  -9  [green/red mini bar]
A  src/ui/commit_panel.rs                     +52 -0  [green only]
D  old/file.rs                                +0  -31 [red only]
```

## Diffstat Mini Bar 仕様

- 固定幅の小さい bar として表示する
- bar の最大セグメント数は **5〜8 程度**
- additions は緑、deletions は赤
- additions/deletions の比率に応じて緑/赤の割合を変える
- 変更行数が 0 の場合は bar を表示しない、または薄い placeholder にする
- total changes が多いほど濃く、少ないほど控えめに表示してもよい
- binary file は `BIN` と表示する
- renamed file は `R` と rename 元/先を表示する
- conflicted file は `U` または warning icon を優先する

## 計算仕様

入力:

```rust
struct FileDiffStat {
    path: PathBuf,
    status: FileStatus,      // 実装では ChangeKind を流用してよい
    additions: usize,
    deletions: usize,
    is_binary: bool,
}
```

bar 表示用の計算:

```text
total = additions + deletions
green_ratio = additions / total
red_ratio   = deletions / total
```

ただし**少量変更でも見えるように、追加/削除が存在する場合は最低 1 segment** を表示する。

例:

- `+10 -0` → 緑のみ
- `+0 -10` → 赤のみ
- `+5 -5` → 緑と赤が半分ずつ
- `+1 -20` → 緑 1 segment + 赤多数
- `+200 -10` → 緑多数 + 赤 1 segment

## 表示場所(優先順位)

1. Right Panel(Inspector)の Changed Files list
2. Commit Panel の staged/unstaged file list
3. Commit Preview(W14-PREVIEW 完了後に PM が統合)
4. Compare View(同上)

## UX 要件

- file path より diffstat が目立ちすぎないこと
- 右揃えで表示し、一覧のスキャン性を高めること
- additions/deletions の数値は monospace または桁揃えすること
- selected row でも読みやすいこと
- compact mode(KAGI_COMPACT)でも表示できること
- tooltip で詳細を表示すること(例: `4 additions, 3 deletions`)

## 実装方針

- **diffstat の計算ロジックと UI 描画を分離**する
- 既存 diff 取得処理(`src/git/diff.rs` の `commit_changed_files` 等)から additions/deletions を
  取れるか確認する。取れない場合は git2 の `Patch::from_diff` + `line_stats()` で per-delta に集計する
  (kagi の git2 0.21 で利用可能。総計しか返さない `Diff::stats()` は使わない)
- UI 側には `FileDiffStat` だけを渡す
- gpui component として `DiffstatMiniBar` を作る(`src/ui/diffstat_bar.rs`)
- unit test で bar segment 計算を検証する
- 色は theme() の semantic color を使う(ハードコード禁止、6 テーマすべてで成立させる)
- 性能: 大 commit では per-file patch 生成がコスト → 既存の MAX_FILES 切り詰めの内側でだけ計算する。
  巨大 blob は is_binary / 上限で逃がし、UI スレッドを塞ぐ場合は既存 async パターンに乗せる

## チケット

| ID | 内容 |
|----|------|
| T-DIFFSTAT-001 | FileDiffStat model を定義する |
| T-DIFFSTAT-002 | commit diff / staged diff から additions/deletions を集計する |
| T-DIFFSTAT-003 | DiffstatMiniBar の segment 計算ロジックを実装する |
| T-DIFFSTAT-004 | DiffstatMiniBar gpui component を実装する |
| T-DIFFSTAT-005 | Changed Files list に status / additions / deletions / bar を表示する |
| T-DIFFSTAT-006 | selected row / compact mode / tooltip の表示を調整する |
| T-DIFFSTAT-007 | binary / renamed / deleted / conflicted file の fallback 表示を実装する |

## 完了条件

- Changed Files list で各ファイルの追加/削除行数が見える
- 右端に緑/赤の diffstat mini bar が出る
- 追加/削除の比率が視覚的に分かる
- binary/renamed/conflicted の例外表示が破綻しない
- diffstat 計算ロジックに unit test がある
