# ADR-0129: Plan モーダルの i18n(構造化 PlanNote への移行 + 実装ハンドオフ)

- Status: Accepted
- Date: 2026-07-19
- Follows: ADR-0091(typed settings / i18n 基盤)、ADR-0121(ハンドオフ型 ADR の前例)
- 背景: plan→confirm モーダルの blockers / warnings / title / recovery は
  kagi-git の ops 層が**英語プローズを直接 format! で生成**しており(実測:
  blocker/warning 144 箇所 + title/recovery 41 箇所、13 op ファイル)、
  表示言語設定(EN/JA)が効かない(user report)。

## Decision

ops は文言ではなく**構造化データ**を返し、表示層が localize する。

1. **kagi-domain に `PlanNote` enum を新設**(`plan.rs`):
   ```rust
   pub enum PlanNote {
       BranchNotFound { name: String },
       BranchUnmerged { name: String, tip_short: String },
       WorktreeDirtyPin { branch: String, path: PathBuf },
       // … 1 メッセージ = 1 バリアント、パラメータはフィールドで持つ
   }
   ```
   `OperationPlan { blockers: Vec<PlanNote>, warnings: Vec<PlanNote>, … }` に変更。
   title / recovery も同様の enum(`PlanTitle` / `PlanRecovery`)か、
   バリアント数次第で `PlanNote` に統合(実装時に判断、無理な統合はしない)。

2. **英語文言はバイト固定の「正本」として維持する(最重要)**:
   - `PlanNote::message_en()` を kagi-domain に実装し、**現行の英語文字列と
     バイト単位で同一**の文言を返す。
   - oplog への記録・`[kagi]` klog 行・headless テストの grep は
     **必ず `message_en()`** を通す。これにより oplog の互換性と klog 契約が
     移行前後で不変になる。
   - UI 表示(モーダル)だけが `Msg` 経由の localize を通す:
     `kagi-ui-core::i18n` に `fn plan_note_text(note: &PlanNote) -> String`
     を置き、`lang == Ja` なら日本語、それ以外は `message_en()`。
     日本語文言はこの関数に集約(Msg enum の肥大を避ける。既存 Msg 規約との
     整合は実装時に判断してよいが、**EN は必ず message_en() に委譲**し
     二重管理しない)。

3. **段階移行**(ADR-0121 方式、1 op ファイル = 1 PR):
   - **Phase 1(テンプレート確立)**: `PlanNote` 機構 + 最小の op
     (`discard.rs`、4 箇所)を変換。`OperationPlan` の型変更は全 ops に
     波及するため、Phase 1 の間は互換 shim
     (`Vec<String> ⇄ Vec<PlanNote::Verbatim(String)>` の `Verbatim` バリアント)
     で未移行 op を包む。
   - **Phase 2(並列 fan-out)**: 残り 12 ファイルを Agent 並列で変換。
     各 PR の合格条件: 対象 op の headless テスト・oplog 文字列が
     **変更前とバイト同一**(message_en が正本である証明)、
     ja 表示のスクリーンショット or モーダル文言を PR に記載。
   - **Phase 3**: `Verbatim` バリアントの削除(全 op 移行済みの証明)+
     CI ゲート: `grep 'blockers.push(format!'` が 0 件。

## 検証

- 各 PR: `cargo test --workspace` 緑 / fmt / clippy 増分なし。
- oplog 互換: 移行 op の plan を JSON 化した oplog エントリが移行前と同一
  (Phase 1 でスナップショットテストを 1 本作り、以後の PR が再利用)。
- klog 契約: `[kagi] refused: … blockers` 系の行がバイト同一。
- GUI: 言語設定 JA でモーダルの blocker/warning が日本語になること(目視)。

## 非目標

- トースト・フッター等モーダル以外の文言(既に Msg 経由のものが多い。
  残りは別 ADR)。
- 文言の改善・言い換え(EN はバイト固定が正義。改善したければ移行完了後)。
