# ADR-0121: Zed-informed modularization policy(S5 の実施方針 + 実装ハンドオフ)

- Status: Accepted
- Date: 2026-07-17
- Follows: ADR-0120(workspace pane framework)、ADR-0117/0118(Entity テンプレート /
  Phase 5.2)、ADR-0075/0095(S5 AppState)、ADR-0072(crate 分割)
- 目的: `src/ui/mod.rs`(4.7k LOC / KagiApp 110+ フィールド)の成長を止める
  **恒久方針**を、zed 本家 v1.0 の実測に基づいて確定する。本 ADR はそのまま
  別マシン/別セッションへの実装ハンドオフとして使える粒度で書く。

## Context — zed v1.0 の実測(2026-07-16、zed main 90b3aa0 にて)

| 指標 | zed 実測値 |
|---|---|
| クレート数 | 237 |
| .rs ファイル数 / 平均 LOC | 1,827 / **807 行** |
| 800 行超ファイル | 448(24.5%) |
| 2,000 行超ファイル | 151 |
| 最大ファイル | `workspace.rs` 16,915 行、`editor.rs` 12,489 行 |
| `Editor` 構造体のフィールド数 | **205** |
| `Workspace` 構造体のフィールド数 | 63 |

**結論: zed はファイル LOC を抑制していない。** kagi の 800 行 ratchet は zed の
実態より厳しい。zed が守っているのは次の 3 つの境界である:

1. **クレート = 機能境界(コンパイル時強制)** — `git_ui` / `project_panel` /
   `editor` / `workspace` / `theme` / `settings` … 依存方向は Cargo が強制し、
   機能間の reach-in が構造的に不可能。
2. **ハブ + trait 拡張点(登録制)** — 中心の `workspace` クレートは
   `Item` / `Panel` trait(`workspace/src/item.rs:170`, `dock.rs:36`)を定義する
   だけで、git_panel 等の存在を知らない。機能クレート側が実装して登録する。
   中心構造体は大きくてよい(Editor 205 フィールド)が、**機能はフィールドでは
   なく Entity としてぶら下がる**ので、機能追加で中心が育たない。
3. **クレート内は sibling ファイルへの impl 分散** — `editor/src/` は 61
   ファイル(`actions.rs` / `clipboard.rs` / `movement.rs` / `element.rs`(描画)
   / `display_map/` …)。同一構造体への `impl` ブロックを関心ごとに別ファイルに
   置く。巨大テストは `editor_tests.rs`(41k 行)として製品コードから隔離。

## Decision

kagi の S5 は「ファイルを小さくする」ではなく「**境界を増やす**」で進める。

- **LOC ratchet は tripwire として維持**(成長の検知器)。ratchet 超過自体を
  リファクタの理由にしない。超過時の選択肢は (a) sibling への impl 分散、
  (b) 意図的な baseline 更新、の二択。
- **機能の主戦場を「KagiApp のフィールド + render 分岐」から「Entity + スロット
  登録」へ移す**(ADR-0120 の枠の実装)。
- **UI クレート分割は trait 境界が安定してから**。境界なしにクレートを切っても
  god-file が引っ越すだけなので、Phase C まで着手しない。

## 実装計画(ハンドオフ)

前提知識: `CLAUDE.md`(必読)、ADR-0117〜0120、
`docs/rearch/migration/README.md`(S5 の位置づけ)。
不変条件: UI に git2 禁止 / `[kagi]` klog 契約 / write 操作の
plan→confirm→preflight→execute→verify→oplog。各フェーズで
`cargo test --workspace` 緑 + `cargo fmt --check` クリーンを維持。

### Phase A — sibling impl 分散(機械的・低リスク・随時)

`src/ui/mod.rs` の `impl KagiApp` を関心ごとに sibling へ移す。既存の
`operations/` / `render_*.rs` / `diff_view.rs`(T-HOTSPOT-UIMOD-001 の前例)と
同じ「**behaviour-preserving relocation**」方式。

移動候補(2026-07-17 時点の mod.rs 内の塊):
- solo(`toggle_branch_solo` / `branch_history_commits` 一式)→ `graph_solo.rs`
- タブ切替(`switch_repo` / `build_tab_view` / `apply_tab_view` 周辺)→ `tabs` 系
- reload 群(`reload_checked` / `reload_async` / `reload_working` …)→ `reload.rs`

規則: 1 PR = 1 塊、diff は移動 + `use` 調整のみ、klog 文言は一切触らない。

### Phase B — ペイン内容の Entity 化 + スロット登録(本丸)

ADR-0120 のスロット枠に沿って、center を占有するオーバーレイ群を順に
`Entity<XView>`(fat entity、ADR-0117 テンプレート)へ:

| 対象 | 現状 | 状態 |
|---|---|---|
| Conflict | `Entity` 化進行中(Phase 5.2 Mechanism B) | 継続 |
| FileHistory | `Option<Entity<FileHistoryView>>` 済 | スロット登録に移行 |
| Ecosystem | `Option<Entity<EcosystemView>>` 済 | 同上 |
| EditorWorkspace | `Option<Entity<EditorWorkspaceView>>` 済 | 同上 |
| MainDiff | `Option<MainDiffView>`(plain struct) | Entity 化から必要 |
| Compare | `Option<CompareView>`(plain struct) | 同上 |

やること:
1. `src/ui/workspace.rs`(ADR-0120)に zed の `Item` 相当の最小 trait を切る。
   zed の定義(`Focusable + EventEmitter + Render`)をそのまま輸入せず、kagi の
   スロット解決(center takeover / center 単独 / right)に必要な最小面
   (render + 生存判定 + 破棄フック)から始める。
2. `render_body.rs` の if/else 優先順位を「スロット → 登録された Entity」の
   解決に置き換える(ADR-0120 の表の順序を保存する)。
3. KagiApp から対応する `Option<...>` フィールドと render 分岐を 1 対象ずつ削る。
   1 PR = 1 対象。headless テストの `[kagi]` 行が同一であることを確認しながら。

### Phase C — UI クレート分割(trait 境界安定後)

Phase B で trait 越しにしか会話しなくなったペインから順に
`crates/kagi-ui-<feature>/` へ。依存方向は
`kagi(bin)` → `kagi-ui-*` → `kagi-ui-core`(trait/theme/i18n)→ `kagi-domain`。
`kagi-git` へは従来どおり Backend 経由。CI に「ui クレート間の横 import 禁止」の
grep ゲートを追加(ADR-0078 と同形式)。

### 非目標

- `mod.rs` を N 行以下にすること自体(zed の workspace.rs は 16.9k 行で健在)。
- Entity 化と同時の挙動変更・見た目変更(必ず別 PR)。
- Phase B 完了前のクレート分割。

## Consequences

- 機能追加の置き場が「mod.rs のどこか」から「新しい Entity + スロット登録」に
  変わり、god-file の成長が止まる。
- ratchet の位置づけが明確になる(検知器であって目標ではない)。
- Phase B の間、スロット解決の二重化(旧 if/else と新枠の共存)が一時的に
  発生する。1 対象ずつ移せば headless 契約で退行を検知できる。

## 検証

- 各 PR: `cargo test --workspace` / `cargo fmt --check` / clippy 警告数が
  baseline(39)から増えないこと。
- Phase B 各段: 対象ペインの headless テスト(`tests/`)と `[kagi]` klog 行が
  変更前後で一致すること。GUI の目視は人間が行う。

## 実施結果(2026-07-18 追記)

Phase A〜C を完走した。最終形:

| Phase | PR | 内容 |
|---|---|---|
| A | #137/#138/#139 | graph_solo / tab_view / reload の sibling 分散(mod.rs 4684→3814) |
| B1 | #140 | `WorkspaceItem` trait + center スロット登録制 |
| B2 | #145 | Right スロット(CommitPanel/Inspector)+ MainDiff Entity 化 |
| B2 | #146 | Compare Entity 化(function-rendered thin adapter) |
| C1 | #148 | `kagi-ui-core`(klog/settings/i18n/theme)+ レイヤリング CI ゲート |
| C2 | #149 | `kagi-ui-ecosystem`(event 2 個で結合を縮約) |
| C3+C4 | #152 | `kagi-ui-file-history` / `kagi-ui-editor`(#150/#151 統合) |

確立したペイン切り出しレシピ(C2 起点):
- データは constructor / `seed_*` で内向きに注入(Backend 呼び出しは bin glue)
- back-call は `EventEmitter<XEvent>` の event enum で外向きに(5 個超えたら
  kagi-ui-core への host-handle trait 導入を検討する — 現時点で必要になった
  crate は無し。editor の 6 個は `*Requested` 2 個が seed 対のため据え置き)
- glue(Backend 連携・oplog・toast・modal)と `WorkspaceItem` アダプタは bin 残置
- 旧パスは `pub use` shim で呼び出し側 diff ゼロ

意図的に bin に残したもの:
- **MainDiff / Compare の描画パイプライン**(`render_diff_list` ほか)—
  FileHistory / Editor の埋め込み diff と共有のため。切り出す場合は
  `kagi-ui-diff` として3者同時に動かす必要があり、費用対効果が出るまで保留。
- `WorkspaceItem` trait 本体(`&KagiApp` 結合)と各アダプタ。
- Conflict 系(Phase 5.2 の Entity flip と合流させるべきで、本 ADR の範囲外)。

CI ゲート(blocking): kagi-ui-* から git2/kagi-git 禁止、kagi-ui-* 間の
横 import 禁止(`invariant-ui-core-layering`)。

## Settings production/browser boundary (#1816, 2026-10-27)

The approved Settings rebuild first extracts a real production control through
the established constructor/`seed_*` and `EventEmitter` seam, and mounts that
same entity in the native host and existing `kagi-web` verification host. Only
then does the same owner expand to the approved eight-pane sidebar design.
This bounded extraction does not complete S5 or authorize a universal UI/Git
facade. Separate boundary and visible-rebuild commits when practical.

`kagi-ui-settings` owns rendering and input only; root host glue resolves
preferences, theme catalogs, provider availability and approvals, performs the
existing native effects, and seeds effective snapshots. It depends only on
domain, UI core, GPUI and gpui-component. The browser fixture records the same
typed intents and supplies deterministic outcomes; it is not a shipping browser
app or another Settings renderer. Native syntax grammar features are selected
only for native targets.

Ordinary setters acknowledge live application, not durable persistence:
the existing store remains the sole writer, with unknown-key preservation,
flat strings, corrupt-file rescue, repeated-key coalescing and quit flush.
Do not invent saved-success notifications, optimistic rollback or a new store
result API. Analyze uses its actual write result: the retained draft and saved
baseline survive pane changes and close/reopen; a delayed result advances only
the saved revision, without replacing subsequent edits. Ordinary reseeding
must not overwrite dirty text. Reset and consent are bound to the invoking
window and canceled when it closes; first enable stays OFF until consent.

Core's atomic theme/language/zoom reads remain shared. Browser initialization
uses nonpersisting loaded-state inputs; native persistence/discovery APIs are
not exposed on wasm. The existing layering gate must also prohibit these host
calls in the shared view on native builds, where Cargo feature unification
alone cannot enforce isolation. Native adapters preserve menu, terminal and
all-window live application and reseed external changes.

The #1816 design records the owner-approved sidebar-vs-toolbar and fixed
category-hue deviations. Preserve ADR-0223 window chrome/lifecycle. Native and
browser exercise the same controls; browser evidence cannot replace native
OS, process, persistence or Git safety verification.

The native implementation retains ADR-0223's 715-point fixed width. The owner's
subsequent compact Settings request explicitly changes initial/content-driven
height to a 640-point ceiling, retaining the 500-point floor where possible and
visible-screen clamping, and changes the theme grid to a horizontal chooser.
Existing frame position/display restoration and storage remain authoritative;
previous auto-saved tall heights are bounded rather than deleting their keys.
The original proposed 900×720 figure is a roomy reference, not current native
geometry. Browser captures do not prove native sizing: native evidence reports
actual dimensions and high-zoom scroll reachability separately. Widening the
window is not a substitute for correct viewport or notification containment.
The shared root takes the flex remainder below fixed host chrome and permits
height shrinkage. Its actual scroll viewport, all sidebar destinations and
notification slot must stay inside the host frame; a full-height body below
the header is not an acceptable substitute for that constraint.

The approved Japanese pane name is `スマートコミット`: this is the localized
feature name, not a translation of the Git `commit` operation. The terminology
gate retains the operation rule and recognizes this named-feature exception.

The browser host embeds the same pinned component icon bytes via the asset
crate's published Cargo build metadata. It must not silently omit symbols or
depend on CDN timing: fixture-only transport/assets differ, production
rendering, control semantics and palette data do not.

### Preserve the final experiment's component contract

The owner's quality baseline is PR #1814's final i10 kit and v9 skill
(`1c6486202fdd28d5c2f600da18baaade645ef5d6`), not the old production Settings.
The port wires production state and input into those roles; matching the pane
inventory or a generic HIG checklist does not authorize reconstructing their
visual vocabulary. `kit_style` owns the original metrics and theme-derived ink;
`controls` owns grouping/navigation/interaction, `control_parts` owns the used
switch/segment/capsule/stepper roles, and `popups` uses the existing GPUI menu.
Pages compose Theme, Layout, Motion and Custom themes instead of drawing parts
or collecting unrelated preferences in Interface.

Carry forward 40/52-point rows, 10-point groups, non-layout one-pixel hairlines,
15/13/11.5-point title/body/secondary hierarchy, label-derived equal segments,
24-point visible controls, capsule geometry and top-lit fixed-hue tiles.
The original theme card is 144 points wide, with an 88-point palette preview,
terminal cursor, eight actual ANSI swatches and a centered 11.5-point caption.
The original small switch is 28×16 with a 12-point knob; the earlier port's
152-point cards and 32×18 switches were not owner constraints.

Do not restore the experiment's known defects: keyboard focus remains a
separate gapped, translucent contrast-safe ring; Light selection is white and
both switch knobs are white. Retain at least 28-point hit targets, selected
indication under hover, real caption ellipses/full accessible names, Japanese
wrapping and no nested cards. Disabled controls and popup options remain inert;
opening or navigating a popup does not emit a preference. Native effect owners,
consent, persistence and Analyze revision handling are unchanged.
Visible summaries and full hover/accessibility descriptions remain separate;
the Analyze input and trailing actions compose one group with a non-layout
separator. Switch motion respects both the effective system policy and the
app preference. Equal segment widths use the actual UI font's shaped labels,
not the experiment's Latin-only character-width estimate.

The requested Light/Dark presentation extends the same theme owner: classify
with `Theme.dark`, mount two labeled horizontal strips, retain one global theme
value and one Light-then-Dark roving group, and keep each strip's browsing state
independent. There are no per-mode defaults, copied gallery or new backend API.

The original 212-point sidebar/32-point rows are not compressed to fit zoom.
Its outer width has a 55% ceiling and can flex below that ceiling to leave the
detail's role-derived minimum: a whole numeric unit plus group insets, or a
144-point theme card plus its full unscaled logical-pixel focus clearance,
whichever is wider.
One existing-style vertical scroll handle owns overflow and keyboard reveal.
Full sidebar names remain accessible and available on hover when truncated.
This preserves the 715-point host/640-point ceiling; it is not a new minimum
window policy or a promise about arbitrarily tiny displays.

The [production construction contract](../ui/modern-ui.md#settings-final-kit-construction-contract)
maps final v9 roles and their rest/hover/pressed/focus/selected/disabled states
to those owners. Preserve the distilled one-shot composition rules, not a
second gallery or an image-only reconstruction. Full-width group/text wrappers
forward natural wrapped sizes; shrink-to-fit trailing controls retain their own
layout. Text → control → row → group containment and actual ellipsis/tooltip
paint are consumer gates. A normal segment is one equal-width rail; Japanese
text may grow its height rather than stacking whole options or clipping.
Analyze's leading status wraps within the group's inner width, while its
wrapping action cluster shares the trailing edge.

GPUI `Pixels` are logical window pixels. Focus stroke/gap/clearance do not scale
with UI zoom, but native backing scale still multiplies them in captured PNGs;
this unit clarification changes no geometry.


