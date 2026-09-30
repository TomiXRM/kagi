# ADR-0206: PR のファイルごとの「確認済み」— head blob に紐づけ、PR ごとに 1 ファイルへ保存する

- Status: **Accepted**
- Date: 2026-10-01
- Related: [#351](https://github.com/TomiXRM/kagi/issues/351)（「viewed 管理」の柱のみ。diff 重畳と suggestion 適用は別 slice）、
  ADR-0136（PR mode のファイル一覧）、ADR-0072（kagi-domain は pure）、#491 / ADR-0191（settings store の temp + rename と破損退避）

## 文脈

大きな PR を何回かに分けて読むと、どのファイルを読み終えたかを覚えておく場所が要る。GitHub の "Viewed" は
ファイルごとのチェックで、PR の head が進んでそのファイルが変わると自動で外れる。Kagi の PR mode には相当する
状態が無かった（#351 棚卸し (c)）。

## 決定

1. **意味**: 1 つの印は「ファイルを見た」という事実と、そのときの **head 側 blob id** の組。ファイルが viewed で
   あるのは、印があり、かつ現在読み込んでいる PR head の blob が印の blob と一致するときだけ。head が進んでも
   blob が同じファイルは viewed のまま、変わったファイルは自動で unviewed に戻る（GitHub と同じ規則）。
   blob が分からない（head 未読込・一覧の head が進んだ直後）ファイルは viewed ではない。判定は
   `kagi_domain::pr_viewed::ViewedFiles`（pure、unit test）。
2. **blob id の出所**: PR mode のファイル一覧は gh ではなく、fetch 済みの `refs/kagi/pr/<remote>/N/head` と base の
   tree diff から作っている。blob id も同じ background 読み取りで head の tree から引く
   （`Backend::blob_ids_at`、read-only、head に無い path は all-zero）。gh の呼び出しは増やさない。
3. **保存先**: `settings.json` ではなく `~/.kagi/pr-viewed/<owner>-<repo>-<pr>.json`（`$KAGI_LOG_DIR` があれば
   その下）。1 PR 1 ファイルで、中身は `{ "<path>": "<blob id>" }` の flat な object。owner / repo が英数字と
   `-` `_` `.` 以外を含む、または `.` / `..` の PR は保存せずメモリ上だけで持つ（ファイル名がディレクトリを
   出ない）。書き込みは settings と同じ temp + rename（`kagi_ui_core::atomic_file`。settings store から移設し、
   `settings:` の klog 文言は変えない）。読めない JSON は空として扱い、`….json.corrupt[.N]` に退避してから
   新しく書く。退避できなければ書かない。
4. **oplog に記録しない**: この PC の閲覧状態であり、repository への write ではない（AGENTS.md 不変条件 4 の対象外）。
5. **UI**: PR 全体のファイル一覧（ADR-0136）の各行に checkbox、ヘッダに「N / M viewed」（JA「N / M 確認済み」）。
   viewed の行は半透明。1 コミットを選んでいる間と Conflicts 表示では出さない（印は PR head の blob に対する
   もので、コミット単位の一覧とは対応しない）。checkbox のクリックは行の選択（diff の切り替え）に伝播しない。

## 却下した案

- **settings.json に入れる**: PR 数に比例して 1 ファイルが肥大化し、無関係な設定の書き込みと競合する。
- **GitHub の viewed 状態（GraphQL `markFileAsViewed`）と同期する**: 書き込み権限とネットワークが必要になり、
  「読んだ」というローカル操作が remote write になる。必要なら別 slice。
- **PR head の commit SHA に紐づける**: head が進むと、変わっていないファイルまで全部 unviewed になる。

## 結果

- 古い印（blob が変わったもの）はファイルに残るが、表示上は unviewed。再度チェックすると新しい blob で上書きされる。
- 検証: kagi-domain unit（一致 / 不一致 / 未登録 / ファイル名）、kagi-ui-core unit（round-trip・破損退避）、
  kagi-git integration（`blob_ids_at`）、Tier A `pr_viewed`（実 PR ref fetch、checkbox クリック → 閉じて再度開いても
  viewed → head が進んで変わったファイルだけ unviewed）。
