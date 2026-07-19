# ADR-0127: Status-bar fetch-age indicator (last fetched timestamp)

- Status: Accepted
- Date: 2026-07-19

## Context

auto-fetch(180 秒間隔)はネットワーク断で**静かに**失敗し続ける
(`auto-fetch: failed (silent)` は klog に出るだけ)。ユーザーはグラフが
古いことに気付けない。要望: 「最後に fetch した時刻」をフッターに出し、
あまりに時間が経っていたら WARN 色でハイライトしたい。

## Decision

1. **時刻源は `.git/FETCH_HEAD` の mtime** — git は fetch のたび(no-op でも、
   CLI からでも)このファイルを書き直すため、「最後にリモートと通信できた
   時刻」として正確・永続(再起動を跨ぐ)・実装が I/O 1 回。
   `RepoSnapshot.last_fetch_secs: Option<i64>` として snapshot 時に読む
   (worktree の private gitdir と commondir の新しい方)。未 fetch は `None`。
2. **表示はステータスバーのチップ** `⇣ <age>`(`42s`/`3m`/`2h`/`5d`)。
   `StatusBarVM` 系の純粋関数 `fetch_age_chip`(ユニットテスト付き)が生成し、
   remote が無い repo・未 fetch では非表示。
3. **閾値 [`FETCH_STALE_WARN_SECS`] = 15 分**(ticker 5 周期分)を超えたら
   `FetchStale` ロール → `color_warning` で描画。
4. **鮮度の担保**:
   - snapshot 由来の値は reload のたび更新。
   - no-op fetch(`changed=false`、reload しない)は成功時に
     `status_summary.last_fetch_secs` を in-place 更新(据え置くと誤警告)。
   - 再描画は `fetch_async` が成功・失敗を問わず毎回 `cx.notify()` する既存
     挙動に乗る(= 3 分粒度で age が進む。無操作でも WARN 遷移が表示される)。
5. SSH リモートビュー(ADR-0089)はローカル FETCH_HEAD を持たないため常に
   非表示(`None`)。

## Consequences

- ネット断で auto-fetch が失敗し続けると、15 分後にフッターの `⇣` が
  WARN 色に変わり、graph が古いことに気付ける。CLI で fetch しても
  (watcher → reload 経由で)正しく更新される。
- チップは既存 StatusBar チップ同様に非 i18n の記号表記(`⇣ 3m`)。
- `[kagi] statusbar:` 契約行は不変(チップは表示のみ)。
