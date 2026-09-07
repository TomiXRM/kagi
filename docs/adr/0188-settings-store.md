# ADR-0188: settings の read-modify-write を単一 store に集約する

## Status

Accepted (2026-09-08) — issue #491、PR #617 レビュー反映。ADR-0091 / ADR-0092
の続き。

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

### 1. 破損原本は必ず保持する (例外なし)

「存在するが read / parse できない」ファイルは `corrupt` として扱う。読み取り
自体は従来通り空 document を返す (設定は best-effort) が、**書き込み時に原本を
退避してから**新しいファイルを書く。

退避先は `create_new` で**排他的に確保**した名前 (`settings.json.corrupt`、
埋まっていれば `.corrupt.1`, `.corrupt.2`, …)。固定名で rename すると、二度目の
破損が一度目の救済原本 (= session 情報の唯一の残り) を置き換えてしまう。確保に
失敗した場合も rename に失敗した場合も、**何も書かない** — 新しい 1 key より
原本のほうが価値がある。変更は pending のまま残る。

判定は load 時のフラグではなく**書き込み直前の再読込**で行う。burst が memory に
溜まっている間 (dirty) に外部が破損させた場合、「memory のほうが新しい」を根拠に
再読込を省くと、破損原本を退避せずに rename で潰してしまう。この例外を作らない。

### 2. 保存は atomic

同一 directory の一時ファイル (`.settings.json.<pid>.tmp`) に書いて `sync_all`
してから `rename` で置換する。中断されても直前のファイルがそのまま残る。
directory の fsync はしていないので、これは crash atomicity であって電源断
耐性ではない。

一時ファイルには**置換先の mode を引き継ぐ**。引き継がないと、ユーザーが
`chmod 600` で絞った `settings.json` が次の設定変更で umask 既定に戻る。
置換先が存在しない場合は何もせず platform 既定のままにする (kagi が独自に
厳しい mode を発明しない)。`.corrupt` の救済は予約した空ファイルの上に原本を
rename するので、救済ファイルは原本の inode = 原本の mode をそのまま保つ。

### 3. 外部変更は content で検出し、自分の key だけ replay する

書き込み直前の再読込で、ファイルが**自分が最後に読み書きした bytes と違えば**
外部変更とみなす。そのときは相手の document を土台に、**この process が書いた
key だけ** (`pending` 集合) を上に載せ直す。相手の key は既知・未知とも残る。
multi-process の last-writer-wins は key 単位まで細かくなるが、無条件の全体
上書きはしない。

drift の判定に `(mtime, len)` を使わないのは、サイズを変えない in-place 編集で
timestamp を戻されると stat では見えないからである (#458 で root cause にした
libgit2 の stat-cache と同じ罠)。読み取り経路では stat を安価な first filter と
して使い、`VERIFY_INTERVAL` (250ms) ごとに実 bytes と突き合わせる。書き込み経路は
常に bytes で比較する。

### 4. coalesce するのは「同じ key の連打」だけ

`schedule_flush` は **直前に書いた key と同じ key** が `FLUSH_WINDOW` (200ms)
以内に再度書かれた場合にのみ保留する。これが drag の形である。別の key の書き込みは
別のユーザー操作なので即座に書く — `session_repos` → `session_active` は
両方ともその場でディスクに乗る。burst 中は 1 本の trailing thread が最終値を
window ごとに 1 回書く。`settings::flush()` を `on_app_quit` に登録して終了時の
取りこぼしをなくす。

### 5. 保留状態は保存先ごとに所有する

store は自分が load した path を持ち、**別 path へ転用されない**。
`KAGI_LOG_DIR` が動いたら新しい store が増えるだけで、古い path の pending は
その store が持ち続ける (path A の書き込みが失敗しても、B へ切り替えた拍子に
捨てられない)。trailing thread も armed された path の store しか触らない。
同時に保持するのは最大 4 file (超えた分は flush してから捨てる)。

## Consequences

- 壊れた `settings.json` はもう消えない。救済先は隣の `settings.json.corrupt`
  (二度目以降は `.corrupt.N`) で、テストが byte 単位の保存を保証している。
- **喪失保証**: 通常の設定変更 (theme / lang / session / zoom / トグル類) は
  同期書き込みなので、強制終了で失われない。遅延しうるのは**同じ key を 200ms
  以内に連打した場合の 2 回目以降**だけであり、実際には divider drag の幅である。
  そのケースでも graceful quit なら `flush()` が最終値を書く。強制終了 (SIGKILL /
  crash) で失うのは drag 中の最新幅 (最大 200ms 分) のみ。
- 書き込みのたびにファイルを 1 回読む。flush は burst 中でも window あたり 1 回
  なので、drag 1 イベントごとの read + parse + write という元の負荷には戻らない。
- 単一 process 内の read-modify-write を直列化する。複数 process が同じ
  `settings.json` を書く場合は key 単位の last-writer-wins になる (相手が触って
  いない key は残る)。
- unit test は `KAGI_LOG_DIR` を触らず、path を取る内部関数
  (`load_doc` / `flush_store` / `write_atomic` / `rescue`) を tempdir 上の実
  ファイルに対して動かす。この crate の他テストが `KAGI_LOG_DIR` を並行で
  set / remove するため。process global と公開 API を通る経路 (stat 不可視の
  外部編集、実 trailing thread、graceful quit、強制終了、path 切替) は
  `settings/store_env_tests.rs` が子プロセスで検証する。
