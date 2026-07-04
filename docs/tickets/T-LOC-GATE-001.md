# T-LOC-GATE-001: ファイル LOC ラチェットを CI に追加する(god-file 再成長防止)

- Status: done
- Group: CI / 予防
- 仕様の正: CLAUDE.md「≤800 LOC/file 目標」、ci.yml の grep gate 前例(ADR-0078)

## 背景(調査済み)

Analyze hotspot の上位は全て 800 LOC 超のファイル(mod.rs 5,829 / conflict_view 2,080 /
commands 1,766 / sidebar 1,610 / theme 1,562 / branch.rs 1,461 …)。
分割チケット(T-HOTSPOT-UIMOD-001 等)で減らしても、歯止めがないと再成長する。
ci.yml には既に「UI git2-free grep gate」の前例があり、同じ形で足せる。

## スコープ

- ラチェット方式: リポジトリに baseline ファイル(例 `ci/loc-baseline.txt`、
  `path max_loc` の平文リスト)を置き、CI ステップで
  「800 LOC 超のファイルが baseline を超えて成長したら fail、縮んだら baseline 更新を促す」
  を bash + wc/awk 数十行で実装。新規ファイルは 800 LOC 上限。
- 対象: `src/**/*.rs` と `crates/**/src/**/*.rs`。テスト・vendor・生成物は除外。
- ci.yml では **advisory(non-blocking)job** に入れる(fmt/clippy と同じ扱い)。
  運用が安定したら blocking 化は別判断。

## 触ってよいファイル

- `.github/workflows/ci.yml`、新規 `ci/loc-baseline.txt`、新規 check スクリプト 1 本。

## 触ってはいけないファイル

- `src/`、`crates/` の Rust コード(このチケットではコードを分割しない)。

## 完了条件

- [ ] baseline 生成コマンドが README コメントかスクリプト内に 1 行で書いてある。
- [ ] 800 超ファイルを +1 行するとローカル実行で fail、縮めると pass することを確認。
- [ ] CI で advisory job として動く。blocking job(test)には影響しない。

## テスト方法

スクリプトをローカルで実行 → わざと 1 ファイル膨らませて fail を確認 → 戻す。

## リスク

- baseline 更新忘れで開発が煩わしくなる → advisory 運用で様子見、fail メッセージに
  更新コマンドをそのまま表示する。

## やってはいけないこと

blocking 化 / 800 未満のファイルへの適用 / Rust コード側の変更 / 複雑な設定ファイル形式。
