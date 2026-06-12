# T-COMMIT-009: Message Template — type/scope/summary/body/test/risk + plain⇄template 切替

- Status: done
- 依存: 既存 Commit Panel(T025〜T027)/ T-COMMIT-007(draft 保存形式の整合)
- 関連: lane W14-TEMPLATE、ADR-0042

## 背景

メッセージ作成の負担軽減。構造化テンプレ(type / scope / summary / body / test / risk)と plain text の
相互切替を提供する。ADR は新設せず Commit Panel UI + 純関数の組み立てで実現。

## スコープ

- template モード: type(選択 or 自由)/ scope / summary / body / test / risk の入力欄。
- **純関数で 1 本の commit message に組み立て**(例: `type(scope): summary\n\n<body>\n\nTest: ...\nRisk: ...`)。
  Conventional Commits 寄りだが厳格強制はしない(空欄は省く)。
- **plain ⇄ template 切替**: template → plain は組み立て結果を Input に流す。plain → template は best-effort
  parse(1行目から `type(scope): summary` を抽出、無理なら summary に全文)。
- 現在の mode は draft(ADR-0042)の `mode` フィールドに保存。復元時はその mode で開く。

## 完了条件

- [ ] template 入力 → 期待する 1 本の message に組み立てられる(空欄は省略)
- [ ] plain ⇄ template 切替で内容が失われない(往復で実用上保持)
- [ ] mode が draft に保存・復元される
- [ ] 組み立て純関数に unit test(複数パターン)
- [ ] `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commit_panel.rs`(UI)/ 組み立て純関数を置くなら `src/git/message_template.rs`(新規・UI 非依存)
- `tests/message_template_test.rs`(純関数のテスト、新規)
- `docs/tickets/T-COMMIT-009.md`

## 触ってはいけないファイル

- `src/git/drafts.rs`(mode フィールドを使うだけ)/ 他チケットのファイル / `Cargo.toml`

## テスト方法

1. `cargo test`(組み立て/parse 純関数)
2. UI は PM がスクリーンショット確認

## リスク・規約

- 切替で入力を失わない設計(往復ロスを最小化)。完全可逆でなくてよいが summary/body は保持
- 文字列処理は `chars()` ベース

## 実装メモ (done)

### 純関数 — `src/git/message_template.rs`(新規・UI 非依存)
- `TemplateFields { type, scope, summary, body, test, risk }`(`Default`)+ `TYPE_CHOICES`
  (feat/fix/docs/… Conventional Commits の定番セット。type 欄は free text も可、chips は quick-pick)。
- `assemble(&TemplateFields) -> String`:
  - subject = `type(scope): summary`。type 無し → scope は捨てて summary のみ(裸の `(scope):` は CC 不正)。
    type 有り・summary 無し → `type:`(往復で type を失わないよう colon を残す)。
  - body / `Test:` / `Risk:` は各ブロックを 1 空行で連結、空欄は省略。Test/Risk は同一ブロックに改行で並置。
  - 全フィールド trim 済み。全空なら空文字列。
- `parse_message(&str) -> TemplateFields`: best-effort。1 行目を `type(scope): summary` として解釈
  (`parse_subject`: colon split + balanced paren scope、type は単語=空白なしの場合のみ採用。
  `Merge branch …` のような prose は type 扱いしない)。最初の空行以降を body。
  非 CC は **全文を summary** に入れて plain→template→plain を可逆に。Test/Risk の再抽出は MVP 外
  (ADR-0042 通り展開済み plain text を真実とし、誤推定を避けて body に残す)。
- テスト: `tests/message_template_test.rs`(新規、20 ケース)— full/部分組み立て・空欄省略・trim・
  type-only colon 保持・parse(type/scope/summary・body 付き・非 CC→summary・prose colon・空)・
  template→plain→template 往復・非 CC lossless 往復。

### UI — `src/ui/commit_panel.rs` は無改変。配線は `src/ui/mod.rs`(commit_panel.rs は state のみ)
- `KagiApp` に `commit_template_mode: bool` と `commit_template_inputs: Option<[Entity<InputState>; 6]>`
  (順序 [type, scope, summary, body, test, risk])を追加。**全フィールド gpui-component InputState**
  (手書き widget 無し)。body は `multi_line(true).auto_grow(2,8)`。両 constructor + `reload()` で初期化/リセット。
- `toggle_commit_template_mode()`: plain→template は plain Input を `parse_message` で分解 → 各 InputState へ。
  template→plain は `assemble` 結果を plain Input へ流す。切替で入力を失わない。
- commit footer に「⇄ Template fields / ⇄ Plain message」トグル。template モード時は 6 欄 +
  type quick-pick chips を描画。色は全て `theme()` 経由。
- `can_commit` / `open_commit_plan_modal` / `start_commit` は `effective_commit_message()`(template 時は
  assemble、plain 時は Input 値)を使用 — template モードでも正しい 1 本の message で commit。

### draft 連携(`src/git/drafts.rs` は無改変、API/`mode` のみ使用)
- autosave(既存 250ms debounce)を流用。保存値は `effective_commit_message()`(template は展開済み plain text、
  ADR-0042)。`save_draft(..., mode)` の mode に現在モードを渡す。6 欄編集も assemble 値の変化で検知。
- モード切替時は `bump_draft_for_mode_change()` で世代を進め、text 不変でも `mode` を確実に永続化。
- 復元(`open_commit_panel`): draft の `mode=="template"` なら message を `parse_message` で再分解し
  template モードで開く。それ以外は従来通り plain Input へ。

### 検証
- `cargo build` green / own-code warning 0(残 clippy 64 件は全て既存他ファイル、新規ファイルは 0)。
- `cargo test` 全 suite pass(message_template_test 20 / drafts_test 含む既存も green)。exit code 0 確認。
- UI スクリーンショットは PM 確認(headless 検証は fixture/tempdir のみ、ユーザー repo 不使用)。
