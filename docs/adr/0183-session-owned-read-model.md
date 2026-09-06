# ADR-0183: snapshot/read を session 単一所有にし active/cache を廃止（#482 段階2）

状態: 採用・実装済み（#482 段階2）
日付: 2026-09-07
関連: [#482](https://github.com/TomiXRM/kagi/issues/482)、
[ADR-0182](0182-session-identity-and-lifetime.md)（段階1）、
[DESIGN §2.2 状態と close](../rearch/app-layer/DESIGN.md)、
[ADR-0095](0095-active-view-single-source.md)、[ADR-0075](0075-appstate-reposession-operationcontroller.md)、
[ADR-0030](0030-async-repo-loading.md)、[#489](https://github.com/TomiXRM/kagi/issues/489)（`RequestSlot`）、
[#286](https://github.com/TomiXRM/kagi/issues/286)（row 再採番）、[#287](https://github.com/TomiXRM/kagi/issues/287)（authoritative reload 優先）

## 決定

snapshot 由来の read model の owner を **session 一つ**にする。
`KagiApp::active_view`（画面のタブ）と `tab_cache`（それ以外のタブ）という
二重所有と、その間の全体 clone を廃止した。

- `src/app/read.rs` の `Reads<V>` が `SessionId → Arc<V>` を持つ唯一の owner。
  タブ切替は**読む key が変わるだけ**で、値の複製は一切発生しない。
- UI は `KagiApp::view()` で借用、`view_mut()` で in-place 更新、
  `publish_tab_view` / `accept_tab_view` で新しい read を発行する。
  view は `Arc` を保持しない（借用のみ）ので `Arc::make_mut` は参照数 1 で
  貫通し、status だけの更新が rows/details を copy-on-write しない。
- `Reads<V>` は **generic**。`src/app` は UI の型名を一度も書かないので、
  `app-layering` gate（`gpui::` 禁止）は慣習ではなく構造で満たされる。
  `TabViewState` の `SharedString` は `Arc<str>` newtype であり、`String` へ
  置換すると clone がむしろ高くつくため据え置いた。read model の型は UI 所有、
  寿命と鮮度は application 所有、という分割にした。

## ブリーフからの意図的な逸脱 — read model は `src/ui` に残した

ブリーフ段階2 は「GPUI 非依存 read model を分離し **WorktreeSession** へ移す」と
書いている。本 PR は `TabViewState` を `src/ui` に残したまま、generic な
`Reads<V>` に格納した。**計画差分であることを明記する。**

理由: `TabViewState` は `CommitRow` / `CommitDetail` 経由で `SharedString` に到達する。
`SharedString` は `Arc<str>` の newtype であり、`String` へ置換すると repo 全体の churn に
なる上に clone がむしろ高くつく。型パラメータにすれば `src/app` は UI の型名を一度も
書かず、`app-layering` gate は慣習ではなく構造で満たされる。read model の **形** は UI、
**寿命と鮮度** は application、という分割にした。

この逸脱で段階3 へ持ち越してよいものと、**持ち越してはいけないもの**を分ける。

| 段階3 へ持ち越してよい | 理由 |
|---|---|
| selection / scroll / focus | 「どの行を見ているか」は表示の状態で、read model の形とは独立（DESIGN §2.2） |
| pane / Entity / draft / menu | GPUI resource であり、そもそも `Reads` には入らない |
| `TabViewState` の物理的な移設先（`src/ui` → session module） | 型の置き場所の話であり、所有権・寿命・鮮度は本 PR で既に application 側にある |
| `reset_per_repo_ui` と Welcome の blank 列挙の残り | 表示 state の集約であり、read の正しさには依存しない |

| 持ち越してはいけない（本 PR で実装） | 理由 |
|---|---|
| **admission との連動** | 型がどこにあっても、admit された write は先行 read を失効させなければならない。`app::admit` が両 admission 点を通す（レビュー項目1） |
| **detach / reattach との連動** | session 寿命と read 寿命は同一。`release_session` と reattach 時の `forget`（レビュー項目4） |
| **full reload の意味処理を落とさないこと** | paging は `amend`（revision を上げない）。conflict 再検出・status baseline を持つのは full reload だけ（レビュー項目3） |
| **loading 表示の owner request への接続** | 表示が第二の真実源だと、read が拒否された瞬間に置き去りになる（レビュー項目2） |

つまり「型の置き場所」は段階3 に送れるが、「所有の意味論」は送れない。本 PR は後者を
すべて実装している。

## 鮮度 — `RequestSlot` を owner scope で再利用

`ReadKey = (SessionId, read revision)`。`SessionId` は incarnation を含むので、
close→同 path reopen は別 owner であり旧 read は届かない。

| 事象 | 挙動 |
|---|---|
| `begin(session)` | revision++ → 新しい read が古い read を supersede |
| `accept(key, v)` | key が現行 slot と一致するときだけ書き込む。不一致は**何も書かず** false |
| `fail(key)` | slot を settle。同じ read を再要求できる（"Loading…" に固着しない） |
| `invalidate(session)` / `invalidate_all()` | mutation admission（`app::admit`）と `Delivery::Invalidate` で revision++。値は残す |
| `amend(session, v)` | request state を触らずに値だけ差し替える。paging 専用 — full reload を supersede しない |
| `has_read(session)` | 初回 read が着いたか。loading placeholder の導出に使う |
| `is_fresh(key)` | slot を取らない read（WIP/working-tree 更新）の判定 |
| `forget(session)` | detach/close で最終参照を解放 |

**admission との連動**: `app::admit` が `Sessions::write_lease`（legacy writer）と
`app::prepare`（remove/stash family）の両方を包み、成功したときだけ
`Reads::invalidate_all` を呼ぶ。lease は全 repository を跨ぐ単一予約
（DESIGN §2.1 の保守的 global busy）なので、失効範囲も開いている全 session に合わせる。
per-`RepoId` admission になったら対象 worktree とその sibling に絞れる
（`ponytail:` コメントで上限を明記）。過剰失効の代償は再 read だけ。

**loading 表示**: `loading_tab` フィールドを廃止し、
`is_loading(session) && !has_read(session)` から導出する。フィールドだった頃は
「tab 切替が置き、その切替が始めた load が消す」構造だったので、初回 load 中の Cmd+R が
その load を拒否した瞬間に placeholder を消す者がいなくなった。導出なら、成功でも失敗でも
slot が空けば placeholder は消える。footer だけは stateful なので、初回 read を置換した
reload が `Busy` を settle する。

**paging**: `load_more_commits` は `publish` ではなく `amend` を使う。paging は画面上の
read の refinement であって repository の新しい観測ではないので、full reload を
supersede してはならない。外部 merge 競合 → watcher full reload → 到着前に Load more、
の順で conflict 再検出が落ちるのを防ぐ。

これで `reload_epoch` と `reload_stale`（#287）を削除した。順序契約は不変:
full reload は古い WIP/read に勝つ（reload が revision を上げ、slot を持たない
WIP read が `is_fresh` で落ちる）。tab 切替 guard（`switch_generation`）は read
からは外れた — read は key に owner を持つので構造的に解決する。
`switch_generation` は operation callback と remote 再 snapshot にだけ残る。

## 所有と表示の分離

背景タブの read が着地しても、その owner のデータだけ更新して**表示は触らない**。
`on_view_published` は「画面上の owner の read が変わった」ときだけ走り、
sidebar fingerprint、#286 の row 再採番 invalidation、background scan の再 arm、
worktree タブ色を更新する。以前は `apply_tab_view` が無条件に走っていたため、
背景タブの refresh が active タブの diff pane を閉じ得た。

副作用として、切替時に「読み込み中の B の裏で A の sidebar が見えている」状態が
なくなった（read が無い owner は空 read model を返し、`loading_tab` が
placeholder を出す）。表示の一貫性としては改善。

## 削除したもの

| 削除 | 置き換え |
|---|---|
| `KagiApp::active_view` | `reads` + `view()` / `view_mut()` |
| `KagiApp::tab_cache` | `Reads` の `SessionId` key（close で `forget`） |
| `apply_tab_view` | `publish_tab_view` / `accept_tab_view` / `on_view_published` |
| `KagiApp::reload_epoch`、`reload_stale` | owner ごとの read revision |
| `show_welcome` の blank 代入 20 行と `KagiApp::with_error("")` の再構築 | tab が無い＝session が無い＝`view()` が空 |
| `KagiApp::loading_tab` フィールド | owner の request slot から導出する `loading_tab()` |
| `main.rs` / `e2e.rs` の手組み `RepoTab` push | `from_snapshot` → `open_initial_tab`（attach してから publish） |

`switch_repo` の instant-apply copy も無くなった。切替そのものが swap である。

## 変えなかったもの

- `[kagi]` 契約行は一字も変えていない。`tab-switch: <name> cached=yes|no` の
  意味は「この session が既に read を持っているか」で、問いは同じ。
- #286 の row 再採番 invalidation、#287 の authoritative reload 優先、
  #488/#562 の同一タブ再選択 no-op と背景 close Keep、#473 の panel 所属判定。
- `busy_op`/global conservative busy、lease、oplog、receipt。段階2 は read だけ。
- selection/scroll/pane/menu/GitHub/cleanup UI と `reset_per_repo_ui` は段階3。

## 検証

- G: `cargo test --workspace`。`src/app/read.rs` の純粋遷移 unit test
  （admit の成功/拒否、初回 load を置換した reload の成功/失敗、paging が
  pending full reload を落とさないこと、reattach の旧 incarnation 解放を含む）に加え、
  実 fixture で `tests/app_read_test.rs`（A/B 逆順完了、同 owner 旧 revision 後着が
  何も書かず新 loading を解除しない、load-more の行再採番、選択 OID と
  `details[row]` の一致、error 後の再 request、実 `write_lease` を通した
  admission 失効と拒否時の非失効、paging が pending full reload を落とさないこと、
  status のみ更新で rows/details が複製されないこと、A→B→A の allocation 同一性）。
- E: `read_owner_ordering`（同ファイル）。初回 Loading 中の Cmd+R が placeholder と
  footer を settle すること、外部 merge 競合 → watcher reload → Load more 先着でも
  conflict 再検出と status baseline が落ちないこと、remote 再 snapshot の反復と close で
  毎回 1 つずつ read が解放されることを実 root で観測する。
- E: `read_owner_switch`（`tests/recovery/read_owner.rs`）。実 root で
  fixture A→B→A ＋ reload / load-more / solo を操作し、owner の rows の
  allocation identity、clone/build/drop カウンタ、行と detail と index の一致、
  close 時の最終参照解放を記録する。RSS 改善値は主張しない。
- `uv run --project ci check-all`、`cargo clippy --workspace`（新規 warning なし）、
  `cargo build -p kagi --features gui-e2e --tests`。
