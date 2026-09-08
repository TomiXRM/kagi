# #627 P0 — 共通ハーネス

> Status: 実装済み。測定は未実施。
> Owner: P0

## 作ったもの

`kagi_git::benchmark` が #627 の headless 共通 owner である。

- `crates/kagi-git/src/benchmark/fixture.rs`
  - seed 固定の synthetic Git template を作る `backend_fixture`。
  - `<fixture>.manifest.json` に schema version、seed、tracked file / commit 数、depth、総 byte 数、最大 directory width、HEAD を出す。
  - template は read-only にし、`materialize_pristine` は `File::create` + byte copy で writable copy を作る。hardlink / reflink を使わず、file / directory / symlink の modified time を template から復元する。
- `crates/kagi-git/src/benchmark/fingerprint.rs`
  - regular file の content SHA-256 と、全 entry の `relative_path / size / modified_ns / mode` を記録する canonical fingerprint。
  - worktree（`.git` を除く）、`.git` entry、`repo.path()` private gitdir、`repo.commondir()` を固定 role として記録する。copy 固有の絶対 path は fingerprint に含めない。同じ canonical gitdir は role を併記して一度だけ走査する。
- `crates/kagi-git/src/benchmark/probe.rs`
  - `ProbeOperation` trait、copy lifecycle、計時、前後 fingerprint、canonical JSON envelope を提供する。
  - CPU 時間は同一 process と reaped child process の合算であり、CLI と libgit2 で同じ範囲を記録する。mutable operation は iteration ごとに materialize し、copy と fingerprint は timer の外側に置く。read-only operation は一 copy を warm series として使う。
  - `backend_probe` は registry dispatcher。P0 は smoke 用 `noop` を登録する。
- `crates/kagi-git/src/benchmark/environment.rs`
  - OS/version、arch、実際に `--git-executable` で選択した Git の version、git2 version、filesystem、`core.fsmonitor`、`core.untrackedCache`、index version、fixture manifest、other-process-load field を JSON envelope に付ける。取得不能値は偽の default ではなく `null`。

## 後続 owner の使い方

```sh
cargo run -p kagi-git --example backend_fixture -- \
  --out "$FIXTURE_ROOT/S" --files 200 --commits 50 --depth 3 --seed 627

cargo run -p kagi-git --example backend_probe -- \
  --repo "$FIXTURE_ROOT/S" --operation <registered-operation> \
  --backend libgit2 --iterations 11 --format json
```

P1–P6 は `ProbeOperation` を実装して自分の module に置く。root registry への追加は §0.1 の
integration owner が wave 後に直列で行う。operation 実装は common copy / timing / JSON / fingerprint
を再実装してはならない。

## Self-test

`cargo test -p kagi-git benchmark` を実行し、次を通した。

1. `pristine_copies_are_independent_after_one_is_mutated`
   - 同一 manifest から別 directory に materialize した二 copy の完全 fingerprint が一致することを先に確認する。続けて一方だけを書き換え、もう一方の fingerprint と template bytes が不変であることを確認する。Unix では source / copy の inode も別であることを固定する。
2. `detects_touch_same_size_rewrite_and_mode_change`
   - content 不変の touch、mode 変更、同一 size の別内容への書換えをそれぞれ fingerprint 差分として検出する。
3. `linked_worktree_records_complete_logical_roots`
   - linked worktree で worktree、`.git` file、private gitdir の `gitdir` / `commondir`、common dir の `objects` / `refs` が fingerprint に入ることを確認する。
4. `cpu_timer_includes_reaped_child_processes`
   - CPU を消費する child process の終了後、probe clock が少なくとも 100 ms 増えることを確認する。
5. `records_the_selected_git_executable_version`
   - fixture executable を `--git-executable` 相当で渡し、その executable が出した version が environment JSON に入ることを確認する。
6. `pristine_copies_preserve_symlink_fingerprint_metadata`
   - fixed modified time を持つ relative symlink を template に追加し、別 directory の二 copy の完全 fingerprint が一致することを確認する。

これらは harness self-test であり、backend 選定用の測定結果ではない。`docs/research/627-backend-verification-plan.md` の §6 / §7 は更新していない。

## Self-test 監査

- copy の完全一致 assertion は root に copy 固有 path を戻す、または file / directory の modified time を復元しない実装で失敗する。後者は `set_copy_modified_time` を no-op にする mutation で実際に失敗を確認した。
- copy 独立性 assertion は hardlink 化で失敗する。inode と、片方の書換え後のもう一方および template の不変性をともに確認する。
- symlink copy は link 自身の timestamp を固定した fixture で検証する。`set_symlink_file_times` を除去する mutation で完全 fingerprint assertion が実際に失敗した。
- touch / rewrite / mode はそれぞれ `modified_ns` / SHA-256 / mode を fingerprint から外すと失敗する。
- linked worktree は role だけを列挙して entries を走査しない実装で失敗する。
- child CPU は、当初の 1 ms threshold では `RUSAGE_CHILDREN` を除去しても失敗しなかったため、workload を増やして 100 ms に引き上げた。`RUSAGE_SELF` のみへの mutation が実際に失敗することを確認した。selected executable version は PATH 上の `git --version` を固定で呼ぶ実装で失敗する。

## Owner 境界

### 後続 owner が触ってよいもの

- 自分の `crates/kagi-git/examples/backend_probe/{a,b,c,d_f,e}.rs` module。
- 自分の fixture scenario module と `docs/research/627/<own-report>.md`。
- operation 固有の canonical output schema と test fixture。

### 後続 owner が触ってはいけないもの

- `crates/kagi-git/src/benchmark/**` の manifest / copy / fingerprint / timing / environment schema。
- `crates/kagi-git/examples/backend_fixture.rs` と `backend_probe.rs` の root dispatcher。
- `docs/research/627-backend-verification-plan.md` の §6 / §7、ADR、PR 本文。
- `tests/gui_e2e_runner.rs` と workflow。

共通 schema に変更が必要なら P0 owner へ戻し、schema version を上げて全 package を揃える。
