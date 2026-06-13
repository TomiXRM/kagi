# T-CONFLICT-UI-004: A/B/Result pane に scrollbar を追加する

- Status: done
- Group: Layout
- 仕様: requirements-conflict-ux.md v2 + ADR-0068(flow)/ 0069(rendering = gpui-component CodeEditor)/ 0070(scroll)

## スコープ

CodeEditor InputState の縦/横 scrollbar(ADR-0069)

## 規約
- Save/Continue/Commit/Abort は別物(ADR-0068)。解決は中央 editor、操作は dashboard/header。
- A/B/Result は gpui-component InputState(CodeEditor)。Zed editor は流用しない(ADR-0069)。
- Plan 経由・in-memory・chars()・theme()・i18n Msg(ours/theirs 非表示)。own-code warning 0。

## 実装メモ (done)
A/B/Result は gpui-component InputState code_editor("text") を使用。code_editor 既定で縦/横 scrollbar + line number 付き。Input::h_full() で pane 高さ充填。

## A/B 縦スクロール同期(ADR-0070)— **deferred(技術的に不可)**
調査結果(gpui-component 0.5.1 `InputState`):
- `scroll_handle: ScrollHandle` フィールドは **`pub(crate)`**(`state.rs:296`)で外部から読めない。
- offset を公開する API(`pub fn scroll*` / `offset` / `set_scroll*`)は **存在しない**(grep 済み)。
- InputState は内部で `ScrollHandle::new()` を生成するため、共有 ScrollHandle の注入も不可。

→ 真の A/B スクロール連動はこのバージョンでは fork なしには実現不可(vendor/Cargo.toml 変更は禁止)。ADR-0070 の「MVP で難しければ各独立に落としてよい(明記する)」に従い、**A/B は独立スクロールのまま**とする(fake しない)。将来 gpui-component が scroll offset API を公開するか upstream に PR が入れば連動を再検討。

## 追記 (line-level rework done)
ADR-0071 により A/B pane を `InputState` から `uniform_list` へ変更したため、上記 deferred は解消。
A/B は共有 `UniformListScrollHandle` を `track_scroll` し、同じ vertical offset でスクロールする。
Result pane は独立 scroll のまま。
