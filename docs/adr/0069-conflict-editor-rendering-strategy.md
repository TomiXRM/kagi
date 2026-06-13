# ADR-0069: Conflict Editor Rendering Strategy(Zed editor 流用可否の調査結論)

- Status: Accepted(2026-06-13、ユーザー依頼の調査 ADR)
- 関連: requirements-conflict-ux.md v2 / ADR-0064 / ADR-0001(gpui 0.2.2 pin)/ ADR-0006(gpui-component)

## 調査: Zed editor component の流用可否

- **gpui 0.2.2(crates.io、kagi の pin)本体に Editor / TextInput は存在しない**。
  elements は div/text/img/list/uniform_list/canvas/svg のみ(調査: `~/.cargo/.../gpui-0.2.2/src/elements/`)。
- Zed 本体の `editor` crate は **GPL-3.0 + zed-git 版 gpui + 内部結合**(ADR-0031 で Reject 済み方針)。
  逐語流用は不可・コスト過大。
- **gpui-component 0.5.1(kagi が既に採用・ADR-0006)に `InputMode::CodeEditor` がある**:
  multi_line / **line number** / **scrollbar** / soft_wrap / indent guides / **syntax highlighter** を持つ
  (`gpui-component-0.5.1/src/input/`)。kagi は既に `InputState` を多用。

## Decision

**Zed editor は流用しない。Conflict Editor の各 pane は gpui-component の `InputState`
(`InputMode::CodeEditor`)で構成する。**

- A / B pane: **read-only** の CodeEditor InputState(monospace + line number + scrollbar)。
- Result pane: **Preview mode = read-only CodeEditor**、**Edit mode = editable CodeEditor**(ADR では
  2 モード、T-CONFLICT-UX-015)。
- selected hunk highlight は InputState の行ハイライト or オーバーレイで実装(詳細は実装で確定)。
- syntax highlight は gpui-component の highlighter を将来差せるが MVP は monospace+行番号で十分。
- 利点: 新規 editor を自作せず、依存純度(gpui 0.2.2 + gpui-component のみ)を保てる。

## Consequences
- 3-pane は InputState×3(A/B 読み取り専用 + Result)。uniform_list 自作描画から InputState へ寄せる。
- InputState は Window 必須(headless 不可)— 既存の lazy-create パターン(sync_modal_inputs)を踏襲。
