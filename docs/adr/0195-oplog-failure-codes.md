# ADR-0195: oplog の失敗理由は構造化した code で持つ

- Status: Accepted
- Date: 2026-09-12
- Related: [#650](https://github.com/TomiXRM/kagi/issues/650)、[#649](https://github.com/TomiXRM/kagi/pull/649)、ADR-0149（oplog）、#500（recovery handle の構造化）

## 文脈

`OpOutcome::Failed { error: String }` は人間向けの散文だけを永続化する。したがって
「なぜ失敗したか」を機械的に識別するには、その文字列を照合するしかなかった。文言は
Git、libgit2、翻訳のいずれが変わっても変わるので、照合は壊れる。

#500 は recovery handle を英語散文から構造化 field へ移した。同じ理由が失敗理由にも
当てはまる。

#649（公開済み rebase の任意コード実行を止める緊急修正）では、typed な
`GitError::RebaseCannotStartWithRepoSettingsDisabled` が recording の時点で文字列に
落ちていた。そこで型を持ち込む設計変更を同時に行うとセキュリティ修正の出荷が遅れる
ため、本件を独立させた。

## 決定

**`FailureCode` を oplog entry の optional field として持つ。**

```rust
pub struct OpLogEntry {
    // ...
    pub failure_code: Option<FailureCode>,
}
```

### なぜ `OpOutcome::Failed` の中に入れないか

`OpOutcome::Failed { error }` を構築する箇所はコード全体で 120 を超える。variant に
field を足すとその全部を書き換えることになり、変更の risk が機構の価値に見合わない。
entry 側に置けば、**構築箇所は 1 つも変わらない**。

失敗理由が entry に属するのはやや緩い表現だが、`backup_refs` と `recovery` が既に
同じ位置にあり、いずれも「失敗した／回復に要る」情報である。一貫している。

### 永続文字列は契約である

`FailureCode::as_str` が返す文字列は、この binary より長く生きる log に書かれる。

- **出荷済みの文字列は変更しない。** Rust の variant 名の変更は自由だが、`as_str` の
  変更は不可
- `shipped_code_strings_never_change` test がこれを声に出す

### 後方互換

- **古い行**: `failure_code` を持たない。`None` として読む。`None` は「失敗しなかった」
  ではなく「この Kagi は code を記録しなかった」を意味する
- **新しい Kagi が書いた行を古い Kagi が読む**: 未知の field は parser が無視する
- **未知の code 文字列**: `FailureCode::Other` として読む。parse は失敗させない。
  新しい版が書いた log を古い版が読めなくなると、log の意味が失われる
- **code を持たない entry の出力は従来とバイト単位で同一**。field は `Some` のときだけ
  出力する

以上を 4 つの test で固定する。

### 表示は変えない

`OpOutcome::Failed` の散文はそのまま残す。人が読むのはそちらで、code はそれを置き換え
るものではない。

## 帰結

- typed `GitError` を持つ recording 経路が code を設定する。持たない経路は `None` の
  まま — 段階的に広げられる
- 文言照合で失敗理由を判定するコードは書かない。判定が要るなら code を足す
- 新しい `GitError` variant を足したら、`From<&GitError> for FailureCode` の網羅
  match がコンパイルエラーで対応を促す
