# T-CONFLICT-005: ResolutionBuffer backend(自動保存)

- Status: backend-done(W26-CONFLICT-CORE。UI レーンは後続)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

ADR-0057: ファイル別 Result 草稿 + 採用元行 metadata + undo 履歴。~/.kagi/conflicts/ へ debounce 保存・復元。unit test

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証

## 実装メモ(backend-done)

- `src/git/resolution.rs::ResolutionBuffer`。`from_repo(repo)` が各衝突ファイルの current/incoming
  side テキストを materialize(`merge_file_from_index` + `style_zdiff3`、不可時は standard/無 draft に
  graceful fallback)。WT/index は execute まで触らない(in-memory)。
- `apply_choice(path, Current|Incoming|BothCurrentFirst|BothIncomingFirst)` / `set_manual_text`。
  per-file undo/redo。per-line provenance `LineOrigin = Current|Incoming|Manual`。
- 自動保存: `~/.kagi/conflicts/<sha1(repo)>/buffer.json`(serde 非依存・手書き JSON、drafts.rs 準拠。
  key は canonicalize で正規化し macOS `/var`↔`/private/var`・workdir 末尾 `/` を吸収)。
  `autosave()`/`load()`/`clear()`。履歴は永続化せず現 Result + side テキストのみ round-trip。
- marker-residue 検査は `checklist.rs::text_has_conflict_marker`(ADR-0043 rule4)を再利用 →
  `files_with_marker_residue()`。
- 行分割は `&str` の `'\n'` split・chars() のみ(バイトスライス禁止遵守)。
- 検証: lib unit(choice/undo/provenance/JSON round-trip/marker)+ `tests/conflicts_test.rs`。
