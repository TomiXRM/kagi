# ADR-0197: tab UI state を session が所有する — #643 Wave 4 の契約

- Status: Accepted (Wave 4 契約; 実装は S1–S6 で分割)
- Date: 2026-09-14
- Related: [#643](https://github.com/TomiXRM/kagi/issues/643) A4、ADR-0196（operation lifecycle contract）、ADR-0183（session-owned read）、ADR-0182（close は実行取消ではない）、ADR-0121（WorkspaceItem）、ADR-0093（one modal at a time）
- 適用範囲: `src/ui/`（`mod.rs` / `tabs.rs` / `tab_view.rs` / `workspace.rs` / 各 pane）

## 文脈

ADR-0183 は **read model** の所有を解決した。`KagiApp::reads: app::Reads<TabViewState>` は
`SessionId` keyed で、tab 切替は「別の key を読む」だけであり copy も破棄もしない。

**presentation intent と resource は解決していない。** `origin/main` = `8131320b` の実測:

- `KagiApp` の field は **121**。
- `src/ui/tabs.rs` の `reset_per_repo_ui()` が tab 切替のたびに約 20 グループ
  （`selected` / `diff_caches` / `wip_diffstat` / `last_working_status` / `github_*` /
  `cleanup_prs*` / `pr_menu` / repo-scoped modal / `conflict*` / `operation_history` /
  `history_seed_attempted`）と `WorkspaceItem::dispose` 9 本を **破棄**する。
- per-repo 状態のうち 7 つが **path stamp**: `github_prs_for`、`conflict_detected_for`、
  `smart_commit_detected_for`、`ecosystem_inflight`、`fetch_in_flight_repo`、
  `terminal_sessions: HashMap<PathBuf, _>`、`file_menu`。ADR-0196 決定 3 は path 文字列を
  locator 以外の identity に使うことを禁じている。
- `switch_generation` の実 consumer は 2 箇所のみ（`operations/branch.rs`、`tabs.rs` の
  remote refresh）。他は doc comment。

帰結は 3 つ。

1. **忘れると漏れる。** per-tab 状態を足す正しさが `reset_per_repo_ui` の網羅性という
   人間の checklist に依存している（構造的に守られているのは
   `ActiveModal::is_repo_scoped` の exhaustive match だけ）。
2. **tab 切替が破壊的。** A→B→A で選択・undo history・conflict 画面・diff cache が消える。
   tab は独立した作業場になっていない。
3. **view の存在が意味を持ちうる。** `conflict.is_some()` のような形が残る。#704 で
   否定したのと同じ hazard class。

#643 A4 の決定は既に「GPUI entity を app core へ移さず、UI host に
`HashMap<SessionId, TabUiState>` を作る。**detach だけがその session の UI resources を
dispose する**」であり、Wave 4 完了条件は「`reset_per_repo_ui` が owner map disposal へ縮小」。

## 決定 1 — 所有の分類（5 種）

field を「型」や「描画位置」ではなく、**意味上の owner と寿命**で分類する。

| 分類 | 定義 | 置き場所 |
| --- | --- | --- |
| per-session | 2 つの open session が互いに異なる値を持てるもの / 特定 session・worktree の選択・位置・cache・evidence・resource | `TabUiState`（`SessionId` keyed） |
| window-global | repository を切り替えても引き継ぐ window chrome、単一 overlay slot、cross-repository service | `KagiApp` |
| process / settings | window を閉じても残す preference、repository 非依存の capability detection | `settings::` / process global |
| operation-owned | 実行中 task、lease、reconcile、supervisor、planning / fetch coordination | `app::Sessions`。**`TabUiState` に入れてはならない**（tab close が実行を取り消してしまう — ADR-0182） |
| memoization / dedup key | owner ではなく「同じ probe / cache を共有できる入力」の identity | 入力が worktree 固有なら `WorktreeId`、machine capability なら global probe revision。`SessionId` へ機械的に置換しない |

主要な個別 ruling:

| 対象 | 分類 | 根拠 |
| --- | --- | --- |
| `selected`、scroll handle 群、`branch_groups_collapsed`、`commit_limit`、`diff_caches`、`wip_diffstat`、`last_working_status`、`operation_history`、`history_seed_attempted`、`repo_session`、pane entity | per-session | 表示内容または resource が session に結び付く。`commit_limit` は load-more の cursor であり、A で伸ばした値を B の初回 snapshot に使う現挙動は owner 不在の singleton に由来する偶発的挙動 |
| `terminal_sessions` | per-session resource | `HashMap<PathBuf, _>` をやめ `TabUiState` の `Option<KagiTerminalSession>` にする。close / detach でのみ drop |
| `bottom_panel_open` / `bottom_panel_height` / `bottom_tab` | window-global | panel は window chrome。PTY だけが per-session。`switch_repo` は `ensure_terminal` を呼ばないので「B へ switch しただけで B の PTY が生える」ことは現状起きない。**この性質を維持する** — session が無ければ placeholder を出し、render / switch を spawn trigger にしない |
| `active_modal` | window-global 1 slot | ADR-0093 の「同時に 1 つ」を tab workspace retention より強い不変として残す。repo-scoped variant は frozen `SessionId` / `Attachment` を持ち、tab departure では park せず閉じる |
| `modal_section_overrides` | window-global | static section id に対する chrome preference。`modal_list_scroll` は tab ではなく現在の modal slot に従属させ、open / replace で reset |
| `fetch_in_flight` + `fetch_in_flight_repo` | operation-owned | fetch は既に `reserve_write("fetch", …)` で lease を取る（`commands.rs` / `remote_branch.rs`）。残件は bool + path の routing なので `Option<FetchFlight { owner, waiters }>` に型を与え、path 比較を消す。`TabUiState` へ入れると close が実行 lifecycle を消せるので不可 |
| `smart_commit` | 分割 | provider / opt-in / model / lang は settings、CLI・Ollama availability は process capability。`smart_commit_detected_for` は owner ではなく memoization key で、probe 入力（PATH / Ollama host）は repo 非依存なので global probe revision へ縮約する |

## 決定 2 — store の形と唯一の disposal seam

```text
TabStores
  reads: Reads<TabViewState>          // observation。background publish が writer
  ui:    HashMap<SessionId, TabUiState> // intent / resource。foreground event が writer
```

**`Reads<TabState { view, ui }>` にはしない。** `accept_tab_view` は whole value を publish
するので、同じ値に intent を入れると背景 read の着地が selection / scroll / entity handle を
過去へ戻す。

lifecycle は次の 3 つに限定し、すべて **`KagiApp::release_session`（`src/ui/tab_view.rs`）を
拡張した 1 本の API** から行う。

- attach: `Sessions::attach` が返した session に ui entry を作る。alias dedup で既存 session が
  返った場合は初期化しない。
- reattach: 旧 incarnation の ui / read を release してから新 `SessionId` を作る。
- detach: `TabUiState` の resource を dispose、`Reads::forget`、`Sessions::detach` を一括。
  **detach は `Sessions::{operations, leases, settled, reconcile}` に触れない。**

機械的不変条件: `dom(ui) = attached sessions`、`dom(reads) ⊆ attached sessions`、
detach 後はどちらにも key が残らない。`attach` / `reattach` / `detach` の production callsite を
tab lifecycle module に限定する（狭い CI rule か field encapsulation）。
「per-repo らしい field」を検出する広い regex rule は**入れない** — 偽陰性を残すだけで
安全性を構造化しない。

`TabUiState` を `src/app::Sessions` に入れる案は不採用。GPUI entity / resource を持つので
ADR-0183 の app-layer purity を壊す。

## 決定 3 — pane entity は retain する（rehydrate しない）

`TabUiState` が pane entity、scroll / list handle、`repo_session`、terminal、
entity-local な編集 / undo state を所有し、**detach / reattach だけが drop する**。
depart / switch では entity を生かしたまま非表示にする。

ADR-0196 が禁じるのは entity の**存在を lifecycle / action availability / admission の
真実源にすること**であり、session-owned resource として保持することではない。両者を
分けるために以下を必須とする。

- entity → root の event、root が起動する read completion、pane の cache 着地は、
  すべて **entity 作成時に凍結した `SessionId` / request token を運ぶ**。callback 内で
  `active_session()` や `repo_path` から owner を再解決しない。
- **retain した data cache は再 activate 時に authoritative ではない。** watcher は active tab に
  1 本しかないので、離れている間に外部変更が起きうる。A へ戻った瞬間に owner の full read を
  開始し、`diff_caches` / `wip_diffstat` / `last_working_status` / conflict content は新しい read で
  revalidate されるまで信用しない（安全側の最小実装は activate 時にこれらの payload を clear）。
  selection / scroll / pane 位置は即復元してよい。
- action admission は常に **accepted read revision + Backend live preflight** に限る
  （ADR-0196 の #704 規範）。
- 大きな再計算可能 payload は per-pane の明示 cache policy で evict してよい。境界は
  「semantic state は retain、再計算可能 cache は測定に基づき限定 eviction」。
  read-only か mutation affordance かでは分けない。

rehydrate 案（plain data から entity を作り直す）を採らない理由: seed が `InputState` の
undo / 選択、list handle、未保存 buffer、pane の request token まで lossless に表す必要があり、
entity と seed の二重 owner を作る。A4 の決定文とも衝突する。

## 決定 4 — 受け入れ oracle

| | 内容 | 位置付け |
| --- | --- | --- |
| leak matrix test | fixture 2 repo で A の観測可能な UI 表面を全部動かし、B が pristine、A に戻ると復元、A を close → 同 path を再 open すると fresh incarnation、B の background completion が active A を触らない | **必須**。各 slice が自分の行を追加する |
| `TabUiState::is_pristine`（test-only probe 可） | `let Self { .. } = self;` を `..` なしの網羅 destructuring で書き、field 追加時にコンパイルを割る。entity は `is_none`、collection は `is_empty`、handle は既定位置 | 補助。root に誤分類された field は検出できないので leak matrix の代替にしない |
| `reset_per_repo_ui` の削除 | 忘れる場所そのものを消す | **完了条件** |
| field 名 regex の CI rule | — | **不採用** |

GPUI の scroll / focus を Tier A から決定的に観測できない state は、その state だけ store の
transition test に分離する。観測できないことを理由に受け入れ条件から落とさない。

## 決定 5 — slice 順（P1 blocker つき）

| slice | 内容 |
| --- | --- |
| S1 骨格 | store と lifecycle API（attach / reattach / detach を `release_session` 1 本へ）、`selected` だけ移管、leak matrix harness の最初の行 |
| S2 identity / callback cutover | `github_*`、cleanup、conflict detector、ecosystem、fetch flight、smart-commit probe を `SessionId` / request token / global revision に分類し直し、path stamp と active-root callback を消す。**`switch_generation` もここで削除** |
| S3 選択と位置 | scroll handle 群、`commit_limit`、`branch_groups_collapsed` |
| S4 read cache と history | `diff_caches`、`wip_diffstat`、`last_working_status`、`operation_history`、history seed |
| S5 pane / resource retention | 決定 3 を `WorkspaceItem` に適用。`dispose(&mut KagiApp)` を session state の close-time disposal へ狭める |
| S6 掃除 | `reset_per_repo_ui` と残った path / generation field を削除、ADR-0196 決定 5 と migration README を更新 |

**S5 を S2 より先に行うこと、および retained entity の callback を captured `SessionId` なしで
root に着地させることは P1 merge blocker。** 背景 tab の entity と subscription が生きたまま
owner を再解決する callback が 1 本でも残ると、inactive B の completion が active A の
pane / modal / footer を書き換えられる。これは現 main の switch-time disposal より安全性が低い。

`switch_generation` の削除は S5 より前に単独で可能。残る 2 consumer は
delete-branch plan（`Attachment` の session + visit と planning tag が既にあり generation は
重複 guard）と remote refresh（対象 remote tab の `SessionId` + owner request slot で accept）。

## 決定 6 — #703 との境界

Wave 4 は #703（abandoned executor の supervisor）より先に開始・land してよい。

- #703: executor / process group、termination proof、lease / reconcile、host-close。
- Wave 4: session-keyed な presentation intent / resource、pane callback routing、tab attach / detach。
- `TabUiState` の detach / drop は write task、lease、reconcile、supervisor handle を
  **所有も解放もしない**。`remote_write` / fetch flight / planning / operation completion は
  operation-owned のまま。

共有 hotspot は `Sessions` と `release_session` 周辺なので、同時に実装する場合は
#703 が `src/app/session.rs`、Wave 4 が UI 側 store / tabs を owner とし、schema 変更は直列に統合する。
