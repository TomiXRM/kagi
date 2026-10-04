# ADR-0206: 遅い read を busy snackbar で説明する(ADR-0086 の amend)

- Status: Accepted(2026-10-01)
- Amends: [ADR-0086](0086-busy-snackbar-sync-icon.md)(busy snackbar)
- Related: [#355](https://github.com/TomiXRM/kagi/issues/355)(このうち「遅延の説明」slice。コマンドキューは対象外)、
  [ADR-0204](0204-operation-queue.md) 決定 6(Draft。本 ADR はそのうち read の説明と Skip だけを先に決める)、
  #289(解放保証の教訓)、#495(text-first diff read)、#633(worktree 容量計測)

## Context

ADR-0086 の busy snackbar は write / plan の latch(`write_busy_op` / `remote_write` / `planning`)だけで
駆動され、ラベル 1 行しか持たない。大きいリポジトリで時間のかかる **read** — ahead/behind の計算、
worktree の状態読み込み、worktree 容量の計測、Analyze の hotspot 走査、大きい diff の読み込み — は
何も表示されず、無言で待たされる。git 自身は `advice.statusAheadBehind` などで **2 秒**を超えたら
「時間がかかっています」と理由を出す。

## Decision

1. **閾値は 2 秒**(`kagi_ui_core::slow_read::SLOW_READ_THRESHOLD`)。定数は 1 つで、settings では
   変えない。計測の起点は **その read の説明対象 phase が始まった時刻**(snapshot は phase ごと)。
2. **対象は 4 種の read に限る**(`SlowRead`): ahead/behind(snapshot 内の `collect_branches`)、
   worktree 状態(snapshot 内の `collect_worktrees`)、Analyze、
   Compare / WIP / File History の off-thread diff read。commit の diff は UI スレッドで同期に読むため
   描画ごと止まり、表示できないので対象外(別 issue)。
   worktree 容量計測(inspection)は 2026-10-04 に外した(#1012): 行ごとの「計測中」表示が
   既にあり、snackbar は邪魔というユーザー要望。計測は background のまま続ける。
3. **表示は既存 busy snackbar への 1 行追記**。write が走っていればラベルは write のまま、無ければ
   read のラベル(例「ahead/behind を計算中…」)を出し、2 行目に
   <理由>だけを EN/JA で出す。前置きの「時間がかかっています:」と「(大きいリポジトリでは <対象> に時間がかかります)」の説明文は出さない(2026-10-04、説明文を置かない方針)。
   新しい modal / AppNotice は作らない。2 秒未満の read は snackbar を出さない(常時走る read で点滅
   させない)。
4. **Skip は「計算をやめて不明として表示する」**。操作のキャンセルではなく、oplog に記録しない。
   Skip できるのは結果に既存の「不明」表示があるものだけ:
   - ahead/behind: `SnapshotProbe::skip_ahead_behind` 以降の branch は `UpstreamInfo::counts = None`。
     sidebar と header は「—」、status bar の chip は「—」、toolbar の ↑N/↓N chip は非表示、
     branch menu の Pull / Push は有効のまま(plan が数え直す)。
   - worktree 容量: 対象外になった(決定 2)。行の「計測中」/「未計測」表示はそのまま。
   - worktree 状態・Analyze・diff は説明のみ(worktree の `wip = None` は clean と区別できない)。
   Skip は一時的で、次の read(reload / fetch 後の reload)は通常どおり数える。
5. **終了は handle の drop で決まる**。read を始めた側が `ReadHandle` を background 作業に渡し、
   その drop(結果・失敗・panic・task の破棄)で read が終わる。完了経路が何を落としても snackbar が
   残らない(#289 の教訓)。UI 側は read ごとに 250 ms の ticker で経過を見直し、閾値を越えたら
   `[kagi] busy: slow <op> after 2s` を 1 回出す。
6. **plan / preflight / verify の入力は Skip しない**。Skip 対象は表示用の read だけで、`None` の件数を
   根拠に mutation を始めない(ADR-0204 決定 6 と同じ線)。

## Consequences

- busy snackbar は write の latch に加え、tab ごとの `TabUiState::slow_reads` からも駆動される。tab を
  切り替えれば表示中の tab の read だけが見える。
- `UpstreamInfo` の件数は `counts: Option<AheadBehind>` になり、「0/0」と「不明」を型で区別する。
  plain snapshot(MCP・reconcile)は Skip しないので従来どおり必ず数える。
- 経過秒数と「中断できません」の文言は、PM 決定の表示形式に合わせて本 slice では出さない。
  ADR-0204 決定 6 の残り(queue と write 側の説明)は Draft のまま。

## Amendment (2026-10-04): lease-holding running writes (#355 stage 1)

- 同じ 2 秒定数と 250 ms の再描画周期で、lease に入った write の
  **lease 予約時刻**からの経過秒数を説明する。plan / click からは測らない。
  timestamp と一回だけの klog は lease record に置き、settlement と一緒に消える。
  Unknown / panic で lease が残るときは reconcile の要求を維持しつつ、
  「実行中」の説明は止める。remote SSH pull は #989 の lease 移行後、
  clone は別途対象とし、この段階では実装しない。
- 既存の snackbar の write ラベルを保ち、理由を op kind から機械的に選ぶ。
  未分類なら汎用文と秒数のみ。ETA / 進捗率 / 「中断できません」は表示しない。
  表示は「<理由> · <秒> s」だけで、前置きの文は付けない。
  write は Skip 不能。read と write が重なれば write の理由と秒数が優先し、
  read の Skip は残してよい。キューおよび running cancel は対象外。
- **対象外: UI thread で同期に動く write。** stage / unstage / hunk の write は
  lease を持つが、その間 UI thread が描画できないため、説明を出せない
  (#995 review)。#355 段階 3(write queue)まで同期のまま。snapshot の作成と
  conflict の continue / skip は #996 で background の guard writer に移り、
  経過秒の説明が出る。
- `[kagi] busy: slow write <kind> after 2s` は開始した write ごとに 1 回。
  実行が終わっても Unknown の lease が保持される場合、admission と
  reconcile の既存契約は変えない。
