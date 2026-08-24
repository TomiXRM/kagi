# ADR-0048: UI 多言語対応(en / ja)

- Status: Accepted(2026-06-13、ユーザー依頼)
- Date: 2026-06-13

## Context

モーダル・ポップアップ・トースト等の**説明文**を読ませたい。ユーザー方針(原文):
「Pull, Push, Branch, Stash, Pop, Undo, Terminal, Commit, filter, amend とかいわゆる
**ドメインワードは英語でいい**。それ以外のモーダルとかポップアップとかに出てくる説明文は
言語を複数用意したい。一旦英語と日本語で OK」

## Decision

### 方式: 依存ゼロの enum キー + match テーブル

- 新 module `src/ui/i18n.rs`:
  ```rust
  pub enum Lang { En, Ja }                 // ACTIVE: AtomicUsize(theme() と同型)
  pub fn lang() -> Lang;
  pub fn set_lang(l: Lang);
  pub enum Msg { OpInProgress, DiscardModalNote, /* … */ }
  impl Msg { pub fn t(self) -> &'static str { match (lang(), self) { … } } }
  ```
- fluent / gettext 等の**外部 crate は使わない**(依存純度規約)。enum なので翻訳漏れは
  コンパイルエラーで検出される
- 引数つき文は `Msg::xxx_fmt(n)` のようなヘルパ fn(format! は呼び出し側でなく i18n 側に置く)

### 対象と非対象

| 対象(翻訳する) | 非対象(英語のまま) |
|------------------|----------------------|
| モーダルの説明文・確認文・recovery 文 | ドメインワード: Pull / Push / Branch / Stash / Pop / Undo / Terminal / Commit / filter / amend / checkout / cherry-pick / revert / discard / worktree / tag / stash 等 |
| トースト・Busy footer・status 文 | ボタン上の操作名(Stage / Unstage / Discard all 等の単語ボタン) |
| 空状態・tooltip・警告文 | 列ヘッダ(BRANCH/TAG · GRAPH · MESSAGE)、SHA、branch 名 |
| メニューの説明的項目(About 等) | ADR-0044 の同意ダイアログ 4 文言(ユーザー指定の verbatim 英文。注記として ja 併記は可) |

- **wave 1 = UI 層(src/ui/)のみ**。`src/git/` の plan blocker/warning/recovery 文字列は
  テスト網が文言を固定しているため **wave 2**(別チケット、test 同時更新)で行う
- 既存 UI に日本語ハードコードが少数ある(「別の操作が実行中です」等)— これらも Msg 化

### 言語の選択・永続化

- 既定: `LANG` / `LC_ALL` が `ja` 始まりなら Ja、それ以外 En
- メニュー View → Language → English / 日本語(✓ 付き、テーマ切替と同型)
- settings.json に `"lang": "ja"` を永続化(theme と同じ手書き JSON 読み書き)
- `KAGI_LANG=en|ja` で override(headless テスト決定性)

## Consequences

- 文字列が i18n.rs に集中し、UI コードは `Msg::Xxx.t()` を参照する形になる
- 翻訳追加は Lang variant + match arm 追加で済む(コンパイラが網羅性を保証)
- wave 2(git 層)着手時は blocker 文言に依存するテストの更新が必要

---

## Addendum (2026-08-24): ドメインワードは英語で統一する — カタカナ表記の廃止

- Status: Accepted
- Scope: JA アーム全部(`crates/kagi-ui-core/src/i18n/mod.rs` の `Msg` と
  `crates/kagi-ui-core/src/i18n/plan/*.rs` の ADR-0129 plan note)

### 背景 — ドリフト

本 ADR の「ドメインワードは英語でいい」という方針は、古い `Msg` アームでは
守られていたが、後から入った PR ダッシュボード / エディタ / cleanup 系と
ADR-0129 の plan note 群でカタカナ化が進み、**同じ画面で表記が割れていた**:

| 語 | 英語のまま | カタカナ |
|---|---|---|
| branch | 14 | 11(+ plan note 多数) |
| commit | 11 | 8(+ plan note 多数) |
| merge | 7 | 15 |
| pull | 4 | 3(すべて「プルリクエスト」) |

例: `BcmCurrentBranch`「現在 branch」の隣に `PrJumpToBranch`
「ブランチへジャンプ」。

なお監査時の「stash / squash / rebase / push / tag / worktree / checkout は
どこでも英語」という前提は **`Msg` に限れば正しいが plan note では成り立って
いなかった**(チェックアウト 26 / タグ 22 / ステージ 31 / リセット 7 /
リベース 1 / フェッチ 2 箇所)。これらも同じ方針で英語に寄せた。

### Decision

**ドメインワードは英語表記で統一する。カタカナ表記は使わない。**

- 対象語: branch / commit / merge / pull / push / checkout / tag / stage /
  unstage / reset / rebase / fetch / stash / worktree ほか、本 ADR 本文の
  「非対象(英語のまま)」欄に挙がる git 用語すべて。
- 日本語の助詞・活用は残す: 「branch を削除」「stage 済み」「commit してください」。
- 和文と英単語の間には半角スペースを入れる(句読点・括弧・引用符に接する側は
  入れない): 「現在の branch は」「merge commit です。」
- 複合語も分解する: マージコミット → `merge commit`、ルートコミット →
  `root commit`、リモートブランチ → `remote branch`、コミットメッセージ →
  `commit メッセージ`。
- 例外: 「プルリクエスト」は `pull request`(GitHub の固有名詞として英語)。

### なぜカタカナ側ではなく英語側に寄せたか

1. **本 ADR の既定方針そのもの**。ユーザー指定の原文が「ドメインワードは英語で
   いい」であり、方針変更ではなく方針の徹底で済む。
2. **変更量が少ない**。カタカナに寄せる場合、`Msg` の英語アーム約 36 箇所に加え、
   一貫して英語だった stash / squash / worktree などもカタカナ化しなければ
   一貫性が保てず、対象は倍以上になる。
3. **UI と一致する**。ボタン・列ヘッダ・メニューは英語(Merge / Pull / Stage)で
   固定されている。本文がカタカナだと、押すボタンと説明文の語が対応しない。
4. **git CLI の出力と一致する**。日本語話者の開発者も `git merge` の英語出力を
   日常的に読む。英語表記のほうが検索・照合しやすい。

### `HistoryMoveDir`(ADR-0129 の型付けの穴を塞ぐ)

`i18n/plan/history.rs::label_ja` は英語文字列 `"Undo"` / `"Redo"` を
match して `_ => "操作"` にフォールバックしていた。型付きの ADR-0129
パイプラインの中で唯一の stringly-typed な穴で、綴りが変わると黙って
「操作」に落ちる。`kagi-domain` に 2 値 enum `HistoryMoveDir` を追加し、
`HistoryNote::{WrongBranch, HeadNotOnBranch}` /
`HistoryTitle::HistoryMove` / `HistoryRecovery::HistoryMove` の
`label: String` をこれに置き換えた。`label_en()` / `label_en_lower()` が
従来の英語出力をバイト単位で再現するので `message_en` の golden test は不変。
JA 側の match は網羅的になり、フォールバックは消えた。

### 演算失敗文言のヘルパ(`i18n/op.rs`)

同時期に、UI が modal と oplog に出していた約 115 個のハードコード英語
失敗文言を `i18n::op_failed(Op, err)` / `op_plan_failed(Op, err)` に集約した。
1 呼び出しごとに `Msg` を生やすのは非現実的なため、**操作**を key にした
`Op` enum(`(en, ja)` ラベル表)を持たせている。ここでも ja ラベルは
上記方針どおり、git ドメインワードは英語のままにする。
