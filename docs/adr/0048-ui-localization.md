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
