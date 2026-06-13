# ADR-0060: 外部 merge tool / terminal への逃げ道

- Status: Accepted(2026-06-13)

## Decision

- Conflict Mode バナーに「Open in external tool」(settings.json の mergetool コマンド、
  $LOCAL/$BASE/$REMOTE/$MERGED 置換)と「内蔵 Terminal で続ける」を常設
- 外部・CLI での解決は watcher + index 再走査で取り込み、進捗へ反映(Mode の出入りも自動)
- 外部 tool 起動は plan 不要(read-only 配布)だが oplog に note を残す
