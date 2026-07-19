# ADR-0129: Plan モーダルの i18n(構造化 PlanNote への移行 + 実装ハンドオフ)

- Status: Accepted
- Date: 2026-07-19(2026-07-20 Codex クロスレビュー反映で全面改訂)
- Follows: ADR-0091(typed settings / i18n 基盤)、ADR-0121(ハンドオフ型 ADR の前例)
- Review: Codex(GPT)クロスレビュー済み — Major 6 点を本版に反映
- 背景: plan→confirm モーダルの blockers / warnings / title / recovery は
  kagi-git の ops 層が英語プローズを直接 format! で生成しており(実測:
  blocker/warning 144 箇所 + title/recovery 41 箇所、plan 文言への参照は
  41 ファイル 467 箇所)、表示言語設定(EN/JA)が効かない。既に
  `localize_plan_blockers`(src/ui/mod.rs)という**英語文字列一致ベースの
  部分 shim** が存在するが、網羅性がなく二重管理になっている。

## Decision

ops は文言ではなく構造化データを返し、表示層が localize する。

### 1. 型設計(Codex M5/M6 反映: 役割別・カテゴリ別)

```rust
// kagi-domain (OperationPlan は kagi-git::ops から kagi-domain へ移す。
// 命名規約どおり kagi-git が shim 再輸出)
pub struct OperationPlan {
    pub title: PlanTitle,          // 1個・必須
    pub warnings: Vec<PlanNote>,
    pub blockers: Vec<PlanNote>,
    pub recovery: Option<PlanRecovery>,  // 復旧コマンド等、構造を持つ
    // …既存フィールド
}

// フラット 100+ バリアントは禁止。カテゴリでネストする。
pub enum PlanNote {
    Common(CommonNote),     // dirty-WT / conflicted など op 横断
    Branch(BranchNote),
    Stash(StashNote),
    History(HistoryNote),
    Worktree(WorktreeNote),
    // …
    Verbatim(String),       // 移行専用。Phase 3 で削除
}
```

実装前に **distinct template の棚卸し**を行い(144 出現 ≠ テンプレート数)、
英語が似ていても日本語で意味が違うものは統合しない。

### 2. 文字列制御の排除(Codex M2 — i18n より優先の safety 決定)

現在 UI が表示文字列を解析して挙動を決めている箇所
(pull/push の no-op 判定 `contains("nothing to push")`、delete-branch の
`recovery.lines().nth(1)` 等)は、**意味的状態に置き換える**:

```rust
pub enum PlanDisposition { Ready, NoOp(NoOpKind), Blocked }
```

不変条件: **no-op 判定・復旧処理・安全判定で表示文字列を参照しない。**

### 3. EN 正本と互換(Codex M3/M7 反映)

- `PlanNote::message_en()` は kagi-domain の純粋関数で、oplog/klog/EN 表示の
  **唯一の English renderer**。移行中は現行文字列と**バイト同一**
  (「永久固定」ではなく“移行中の契約”。移行完了後の EN 改善は自由)。
  動的値・改行・引用符・path を含む golden test を持つ。
- **oplog の on-disk schema は `blockers: [String]` のまま変えない。**
  `OperationPlan` → oplog の境界で `message_en()` に変換する。
  reader は文字列専用のまま(非文字列要素を黙って捨てる現行 reader を
  構造化 object で踏み抜かない)。テスト: 移行前に生成した literal JSONL
  fixture の読み出し + 新旧行混在 JSONL の読み出し。
- UI 表示のみ `kagi-ui-core::i18n::plan_note_text(&PlanNote)` で localize。
  EN は必ず message_en() へ委譲(二重管理禁止)。日本語文言は
  `i18n/plan/{common,branch,stash,…}.rs` に feature 分割(i18n.rs は既に
  1.7k LOC — これ以上単一ファイルに足さない)。
- 既存の `localize_plan_blockers` / `localized_blockers` 二重状態
  (Codex M8)は Phase 3 までに削除。全 renderer が plan_note_text() を
  直接呼ぶことが完了条件。

### 4. 段階移行(Codex M1 反映)

- **Phase 1(横断 cutover)**: `OperationPlan` の型変更 + 全 producer を
  `Verbatim` で機械的に包む + 全 consumer(renderer/oplog/klog/テスト)を
  新型へ切替 + **discard を最初の構造化 producer に変換**(実例)。
  1 PR だが挙動は不変(oplog/klog バイト同一で証明)。
- **Phase 2(並列 fan-out)**: 残り op ファイルを 1 ファイル = 1 PR で構造化。
  合格条件: 対象 op の headless/oplog 文字列がバイト同一 + JA モーダル文言を
  PR に記載。
- **Phase 3(完了の証明)**: `Verbatim` バリアントと String 変換 API を
  **削除して workspace がコンパイルする**こと(grep ではなく型で証明 —
  Codex M4)。`localize_plan_blockers` 削除もここ。補助 CI として
  `Verbatim`/`From<String>` のゼロ件 grep を置く。

## 検証

- 各 PR: cargo test --workspace 緑 / fmt / clippy 増分なし。
- oplog 互換: 旧 JSONL fixture + 混在 JSONL の読み出しテスト(Phase 1 で作成)。
- klog 契約: `[kagi] refused: …` 系がバイト同一。
- message_en golden test(動的値・改行・引用符・path)。
- GUI: JA 設定でモーダルが日本語になること(目視)。

## 非目標

- モーダル以外の文言(トースト・フッター等)— 別 ADR。
- EN 文言の改善(Phase 3 完了後に解禁)。
- Fluent/ICU の導入(3言語以上になったら UI crate 側で再検討。
  導入しても構造化 args の必要性は消えない — Codex 代替案評価より)。
