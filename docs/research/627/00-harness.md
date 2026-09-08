# #627 P0 — 共通ハーネス

> Status: 実装済み。測定は未実施。
> Owner: P0

## 作ったもの

`kagi_git::benchmark` が #627 の headless 共通 owner である。

- `crates/kagi-git/src/benchmark/fixture.rs`
  - seed 固定の synthetic Git template を作る `backend_fixture`。
  - `<fixture>.manifest.json` に schema version、seed、tracked file / commit 数、depth、総 byte 数、最大 directory width、HEAD を出す。
  - template は read-only にし、`materialize_pristine` は `File::create` + byte copy で writable copy を作る。hardlink / reflink を使わない。
- `crates/kagi-git/src/benchmark/fingerprint.rs`
  - regular file の content SHA-256 と、全 entry の `relative_path / size / modified_ns / mode` を記録する canonical fingerprint。
  - worktree（`.git` を除く）、`.git` entry、`repo.path()` private gitdir、`repo.commondir()` を別 root として記録する。同じ canonical gitdir は root label を併記して一度だけ走査する。
- `crates/kagi-git/src/benchmark/probe.rs`
  - `ProbeOperation` trait、copy lifecycle、計時、前後 fingerprint、canonical JSON envelope を提供する。
  - mutable operation は iteration ごとに materialize し、copy と fingerprint は timer の外側に置く。read-only operation は一 copy を warm series として使う。
  - `backend_probe` は registry dispatcher。P0 は smoke 用 `noop` を登録する。
- `crates/kagi-git/src/benchmark/environment.rs`
  - OS/version、arch、Git / git2 version、filesystem、`core.fsmonitor`、`core.untrackedCache`、index version、fixture manifest、other-process-load field を JSON envelope に付ける。取得不能値は偽の default ではなく `null`。

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
   - 同一 template から二 copy を materialize し、一方だけを書き換えた後、もう一方の完全 fingerprint と template bytes が不変であることを確認する。Unix では source / copy の inode も別であることを固定する。
2. `detects_touch_same_size_rewrite_and_mode_change`
   - content 不変の touch、mode 変更、同一 size の別内容への書換えをそれぞれ fingerprint 差分として検出する。
3. `linked_worktree_records_worktree_git_entry_private_and_common_roots`
   - linked worktree で worktree / `.git` file / private gitdir / common dir の各 root が記録されることを確認する。

これらは harness self-test であり、backend 選定用の測定結果ではない。`docs/research/627-backend-verification-plan.md` の §6 / §7 は更新していない。

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
