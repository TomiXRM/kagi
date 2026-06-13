# T-BCM-030: Merge branch into current branch の operation plan を実装する

- Status: done
- Group: Integrate
- 仕様の正: docs/requirements-branch-context-menu.md + ADR-0049〜0055

## スコープ

ADR-0052: in-memory conflict 予測=blocker、ff 可否表示、dirty warning + stash 提案。文言 `Merge <t> into <c>`

## 完了条件

- [ ] 上記スコープ + `cargo test` 全パス + own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記(担当 lane が更新)

## 規約

- 操作 handler の二重実装禁止(ADR-0049)。fixture / tempdir のみで検証
- 文字列は chars() ベース・バイトスライス禁止(split_at 含む)。色は theme() 経由
- UI 説明文は i18n の Msg 経由(ADR-0048。ドメインワード・branch 名は英語のまま)

## 実装メモ(Codex / w25-bcm-int)

- `plan_merge_branch` / `execute_merge_branch` を追加。local/remote-tracking ref を target として `Merge <target> into <current>` を表示し、ff 可否と merge commit 予定を plan に出す。
- plan は `merge_commits` の in-memory dry-runで conflict を blocker 化し、preview files を生成。execute は ff/merge commit とも checkout_tree → ref move の順序を保持。
- `tests/branch_menu_ops_test.rs` に ff / non-ff / conflict-blocker fixture を追加。
