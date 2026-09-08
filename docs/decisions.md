# Decision Log

> **Status:** Active — append-only  
> **Last updated:** 2026-09-08

ADR にするほどではないが、再計測や同じ失敗を避けるために残すべき決定と実測事実のログです。ADR を置き換えるものではありません。

## 運用ルール

- 新しい行をテーブルの先頭に追加します。初期エントリを除き、過去の決定を遡って追加しません。
- ADR がある決定は `Why` から ADR にリンクします。記入は機械検査しません。強制して中身のない行を増やすより、再判断に必要な根拠を残すことを優先します。
- 上書きされた決定は削除しません。新しい行を上に追加し、古い行の `Why` に新しい行への参照と、元の判断理由および変更理由を残します。
- リリースごとにそのリリース分を `docs/decisions/v0.3x.md` へアーカイブし、このファイルには常に直近 1 リリース分だけを残します。
- 決定日が確認できない初期エントリには確認日 `2026-09-08` を使い、`Why` に確認日であることを明記します。

| Date | Decision | Why |
|---|---|---|
| 2026-09-08 | dirty Pull の衝突予測は `git2::merge_file` で実際に 3-way merge を試す | 「同じパスが両側で変わった」だけでは衝突とは限らない。上流が先頭行、ローカルが末尾行を変えた実測ケースでは自動 merge が成功した。外れる警告はユーザーに警告を読ませなくするため、content merge が判定できない binary・mode/type 変更・片側の追加削除だけを「可能性」として扱う（[#625](https://github.com/TomiXRM/kagi/issues/625)、[ADR-0192](adr/0192-dirty-pull-conflict-preview.md)）。 |
| 2026-09-08 | `gh issue list` は必要件数を覆う `--limit` を明示する | `gh issue list --help` で既定値が 30 と確認された。31 件以上あっても既定実行は警告せず 30 件で打ち切るため、全件を前提にした棚卸しでは件数不足を検知できない。決定日は確認日。 |
| 2026-09-08 | GUI E2E runner は必ず `KAGI_GUI_E2E_ONLY` でシナリオを絞る | 絞らない実行はシナリオごとに native window を開き、約 1,400 窓で macOS の WindowServer を落としてマシンを再起動させた（[#555](https://github.com/TomiXRM/kagi/pull/555)）。`check-e2e-window-helper` は窓の確保経路だけを検査し、実行範囲は制限しない。 |
| 2026-09-07 | Cargo の `target/` は worktree ごとに分離し、共有しない | 2026-09-07 の独立再現で、共有 target 内の `kagi-domain`・`kagi-git`・`kagi` artifact が衝突し、Cargo が stale rlib を `Fresh` と再利用して新規 module に E0432 を出した。clean build が約 3–4 GB/worktree になるディスク費用は、この誤再利用を防ぐため意図的に受け入れる（[#520](https://github.com/TomiXRM/kagi/pull/520)）。 |
| 2026-09-07 | libgit2 の clean 判定を壊す fixture 書き換えは、mtime の秒境界を越すかファイルサイズを変える | libgit2 1.9.4 の index stat cache は、同一秒内に同一サイズで書き換えた working-tree file を clean と報告しうる。blob は変わっていても status が変わらず、#458 の flake になった。fixture の `beta` → `BETA` を `BETA!` にしてサイズを変えると再現しなくなった（[#458](https://github.com/TomiXRM/kagi/issues/458)）。 |
| 2026-09-06 | CI invariant は shell の `grep` ではなく `uv run --project ci` の Python gate で実装する | BSD grep は bounded repetition を 255 で打ち切るため、GNU grep では有効な長いパターンが macOS では何も検査せず成功しうる。実際に #454 で gate が 0 files checked のまま緑になり、人間が発見したため `check-shell-hygiene` を設けた。 |
| 2026-07-21 | cosmic-text 0.19 の Han-unification patch は upstream PR #522 が merge され、GPUI が取り込む release に入るまで維持する | `ja-JP` でも Simplified Chinese glyph が選ばれる fallback path を直す 1 commit patch で、0.14 から 0.19 へ rebase 済み。2026-09-08 時点で [upstream PR #522](https://github.com/pop-os/cosmic-text/pull/522) は open。merge だけで外すと GPUI の選択版には未収録なので、収録 release への更新時に `[patch.crates-io]` を撤去する（[ADR-0130](adr/0130-bundled-japanese-font-fallback.md)）。 |
| 2026-06-12 | gpui 0.2.2 は `runtime_shaders` feature を有効にする | gpui 0.2.2 の `build.rs` は通常 `xcrun metal` で Metal shader を build 時 compile し、Command Line Tools のみの macOS では `metal` が無く失敗する。`runtime_shaders` は compile を実行時へ移し、full Xcode 依存を避ける（[ADR-0001](adr/0001-gpui.md)）。 |
