# ADR-0187: settings の read-modify-write を単一 store に集約する

## Status

Accepted (2026-09-08) — issue #491。ADR-0091 / ADR-0092 の続き。

## Context

`settings.json` の読み書きは key ごとに「全ファイル load → 変更 → 全ファイル
save」だった (`crates/kagi-ui-core/src/settings.rs`, 監査 revision
`5a5fd0f0`)。ここに 3 つの問題があった。

1. **破損ファイルが黙って消える。** `Settings::load` は read / parse の失敗を
   `Settings::default()` (空) に変換していた。そのまま次の設定変更が走ると、
   空の document が原本の上に書き戻される。失うのは「他の key」だけではない:
   `session_repos` / `session_active` も同じファイルにあるので、次回起動時に
   復元されるタブ集合ごと消える。1 文字壊れた JSON が、ユーザーの作業状態を
   全部持っていける経路だった。
2. **保存が atomic でない。** `std::fs::write` は対象ファイルを直接 truncate
   する。書き込みが途中で止まれば、残るのは切り詰められた (= 次回 parse 不能な)
   ファイルであり、1 の経路に直結する。
3. **高頻度 I/O。** column divider の drag は 0.5px 動くたびに
   `theme::set_col_width` → `write_setting` を呼ぶ。1 イベントごとに
   read + parse + serialize + write が 1 往復していた。

## Decision

`crates/kagi-ui-core/src/settings/store.rs` を **read-modify-write の唯一の
所有者**にする。公開 API (`Settings` / `read_setting` / `write_setting`) と
on-disk 形式 (flat な string 値の object、unknown key 保持) は変えない。

1. **parse 失敗を default に変換して書き戻さない。** 「存在するが read /
   parse できない」ファイルは `corrupt` として記録する。読み取り自体は従来通り
   空 document を返す (設定は best-effort) が、**書き込み時にまず原本を
   `settings.json.corrupt` へ rename** してから新しいファイルを書く。rename に
   失敗したら何も書かない — 新しい 1 key より原本のほうが価値がある。
   `klog!("settings: parse failed, original preserved: {e}")` と
   `klog!("settings: corrupt file kept at {path}")` が診断になる。
2. **保存は atomic。** 同一 directory の一時ファイル (`.settings.json.<pid>.tmp`)
   に書いて `sync_all` してから `rename` で置換する。中断されても直前のファイルが
   そのまま残る。directory の fsync はしていないので、これは crash atomicity
   であって電源断耐性ではない。
3. **parse 済み document を process 内で保持する。** 読みは memory から返し、
   ファイルの `(mtime, len)` が変わったときだけ read + parse し直す
   (GUI E2E runner のように外から `settings.json` を書き換える経路を壊さない
   ため)。書き込みは memory を更新し、直前の書き込みから 200ms 以上空いていれば
   **同期的に**書く (通常の設定変更の durability は従来どおり)。burst 中は
   memory だけを更新し、burst あたり 1 本の trailing thread が最終値を 1 回
   書く。`settings::flush()` を `on_app_quit` に登録し、終了時の取りこぼしを
   なくす。

`KAGI_LOG_DIR` が動いた場合は、**古いファイルに対して**保留中の書き込みを
flush してから新しい path を load する (store は自分が load した path を持つ)。

## Consequences

- 壊れた `settings.json` はもう黙って消えない。復旧手段は隣の
  `settings.json.corrupt` であり、テストが byte 単位の保存を保証している。
- 設定変更 1 回の durability は変わらない (burst の先頭は同期書き込み)。burst の
  末尾のみ最大 200ms 遅延し、graceful quit では `flush()` が拾う。強制終了時に
  失うのは drag 中の中間値だけ。
- 単一 process 内の競合のみを対象とする。複数の kagi process が同じ
  `settings.json` を書く場合、last-writer-wins は変わらない (atomic 置換なので
  壊れはしないが、片方の変更は消えうる)。
- テストは `KAGI_LOG_DIR` を触らず、path を取る内部関数
  (`load_doc` / `flush_store` / `write_atomic`) を tempdir 上の実ファイルに
  対して動かす。この crate の他テストが `KAGI_LOG_DIR` を並行で set / remove
  するため。
