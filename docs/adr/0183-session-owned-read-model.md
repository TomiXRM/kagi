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

## 鮮度 — `RequestSlot` を owner scope で再利用

`ReadKey = (SessionId, read revision)`。`SessionId` は incarnation を含むので、
close→同 path reopen は別 owner であり旧 read は届かない。

| 事象 | 挙動 |
|---|---|
| `begin(session)` | revision++ → 新しい read が古い read を supersede |
| `accept(key, v)` | key が現行 slot と一致するときだけ書き込む。不一致は**何も書かず** false |
| `fail(key)` | slot を settle。同じ read を再要求できる（"Loading…" に固着しない） |
| `invalidate(session)` | mutation admission / `Delivery::Invalidate` で revision++。値は残す |
| `is_fresh(key)` | slot を取らない read（WIP/working-tree 更新）の判定 |
| `forget(session)` | detach/close で最終参照を解放 |

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

- G: `cargo test --workspace`。`src/app/read.rs` の純粋遷移 unit test に加え、
  実 fixture で `tests/app_read_test.rs`（A/B 逆順完了、同 owner 旧 revision 後着が
  何も書かず新 loading を解除しない、load-more の行再採番、選択 OID と
  `details[row]` の一致、error 後の再 request、mutation admission による失効、
  status のみ更新で rows/details が複製されないこと、A→B→A の allocation 同一性）。
- E: `read_owner_switch`（`tests/recovery/read_owner.rs`）。実 root で
  fixture A→B→A ＋ reload / load-more / solo を操作し、owner の rows の
  allocation identity、clone/build/drop カウンタ、行と detail と index の一致、
  close 時の最終参照解放を記録する。RSS 改善値は主張しない。
- `uv run --project ci check-all`、`cargo clippy --workspace`（新規 warning なし）、
  `cargo build -p kagi --features gui-e2e --tests`。
