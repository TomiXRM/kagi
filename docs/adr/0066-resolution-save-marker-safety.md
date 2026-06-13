# ADR-0066: Resolution Save and Marker Safety

- Status: Accepted(2026-06-13)
- 関連: requirements-conflict-ux.md §2.5/3.4 / ADR-0043(marker 検出再利用)/ 0057 / 0067

## Decision

Save / Continue の前に **安全チェック**を必ず通す。`<<<<<<<` `=======` `>>>>>>>` の **marker
残存は blocker**(W26 で checklist の marker 検出を再利用済み)。チェック項目:

- conflict marker 残存(blocker)
- unresolved index entries(stage 1/2/3 が残っている)(blocker)
- empty result(空ファイルになった — 意図か確認、warning→確認)
- binary conflict 未解決(blocker、choose を要求)
- deleted file の判断未了(keep/restore 未選択)(blocker)

- **Save** は ResolutionBuffer(ADR-0057)へ永続化 + result を「resolved candidate」に。
  この時点で WT/index を確定させない(in-memory 主義)。実際の index 書き込みは Continue 時
- **marker 検査は Save 時に warning、Continue 時に blocker**(Save は途中保存を許すが、
  continue は marker 残では絶対に進ませない)
- 検査は純関数 + unit test(marker/各 blocker パターン)
