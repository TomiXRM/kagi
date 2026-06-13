# ADR-0058: Conflict 用語(ours/theirs 廃止)

- Status: Accepted(2026-06-13)

## Decision

- requirements-conflict-ux.md §2 の表を規範とする: 役割 + 実名の2行ラベル
  (merge: Current branch `main` / Merging in `feature/x`、rebase: New base / Your commit being
  replayed、cherry-pick: Commit being applied、revert: Changes being undone)+ Base / Result
- rebase の ours/theirs 反転は `Repository::state()` で文脈翻訳(UI は役割で固定)
- hunk ボタンも役割語(Keep current (`main`) / Take incoming (`feature/x`) / Keep both (current first))
- marker 表示は zdiff3(Base 文脈つき)。i18n は Msg(ADR-0048)、実名は翻訳しない
