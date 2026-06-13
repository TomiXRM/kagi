# T-CONFLICT-UX-010: A/B pane header に accept checkbox を移動する

- Status: done
- Group: Actions
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

各 pane header に採用チェック(☑A=current / ☑B=incoming / 両方=both)。GitKraken 風

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
~~各 pane header に accept toggle(file-level)~~ → **per-hunk へ移行(UX-012)**。pane header の file-level toggle と both-order strip は撤去。代わりに A·B 行と Result の間に**スクロール可能な hunk control list**(`hunk_controls`/`hunk_row`)を置き、hunk ごとに ☑Accept current / ☑Accept incoming / ☑両方(cf/if)を独立トグル(再押しで Unresolved)。focused hunk は `conflict_selected_hunk`(EditorChrome 経由)で選択ハイライト、行クリックで focus 追従。orphan になった `accept_toggle`/`both_order_strip`/`both_button`/`Msg::EditorAccept`/`EditorBothLabel` は削除(own-code warning 0)。
