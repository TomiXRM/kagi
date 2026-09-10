# Decision Log

> **Status:** Active — append-only  
> **Last updated:** 2026-09-10

ADR にするほどではないが、再計測や同じ失敗を避けるために残すべき決定と実測事実のログです。ADR を置き換えるものではありません。

## 運用ルール

- 新しい行をテーブルの先頭に追加します。初期エントリを除き、過去の決定を遡って追加しません。
- ADR がある決定は `Why` から ADR にリンクします。記入は機械検査しません。強制して中身のない行を増やすより、再判断に必要な根拠を残すことを優先します。
- 上書きされた決定は削除しません。新しい行を上に追加し、古い行の `Why` に新しい行への参照と、元の判断理由および変更理由を残します。
- リリースごとにそのリリース分を `docs/decisions/v0.3x.md` へアーカイブし、このファイルには常に直近 1 リリース分だけを残します。
- 決定日が確認できない初期エントリには確認日 `2026-09-08` を使い、`Why` に確認日であることを明記します。

| Date | Decision | Why |
|---|---|---|
| 2026-09-10 | status の遅さの正体は index stat cache であり、backend の選択ではない | libgit2 は index の stat data と一致しない file の内容を毎回ハッシュし直し、`git status` と違って refresh した stat data を**書き戻さない**。50,000 file の repo で warm 135 ms、全 file を `touch` した直後 3,305 ms、その次の scan も 3,318 ms(自己修復しない)、端末で `git status` を 1 回打つと 143 ms。24 倍を永久に払う。#627 で libgit2 が CLI の約 3 倍遅く見えていたのは、CLI が index を黙って修復し libgit2 がしなかったため。P4/P5 が独立に再現し、**20k でも** stale→warm で libgit2 1179.8→77.5 ms、CLI 423.8→61.2 ms。warm index で揃えた 50k の差は 1.3–2.8 倍。修正は `GIT_STATUS_OPT_UPDATE_INDEX`([#657](https://github.com/TomiXRM/kagi/pull/657))。下の 2026-09-10『50k(L) fixture の status 実測は median が再現し』の行は、交絡が無いと結論した点で誤り。交絡は実在したが機構が FS cache ではなく index stat cache だった。warm 直読の median が P4 の値と一致したのは、両方とも stale index を測っていたため（[#655](https://github.com/TomiXRM/kagi/issues/655)）。 |
| 2026-09-10 | 50k(L) fixture の status 実測は median が再現し、採否根拠は 20k までの傾きに置く | `/tmp/kagi-627-p4/L`(tracked=50,000、286MB)を copy せず warm 直読した 10 回で median 約 1,272 ms・range 1,225–1,620 ms。P4 の既存 CLI 50k median 1,317 ms とほぼ一致するため、copy による cold cache が 50k を交絡していたという仮説は実測で支持されなかった。P5 が観測した range 上限 3,988 ms は他プロセス負荷で説明する。50k は median 比ではなく裾の大きさとして記録し、採否は 20k までの傾きを根拠にする（[#627](https://github.com/TomiXRM/kagi/issues/627)）。 |
| 2026-09-10 | fsmonitor の 9.739 ms は 20k fixture の native Git 値であり、50k の CLI 値ではない | この 2 つを取り違えて 50k の全実測値を撤回しかけた。L の index に FSMN extension は無く、L は fsmonitor 測定ではない。実測値を引用するときは fixture 規模と fsmonitor の有無を必ず併記する（[#655](https://github.com/TomiXRM/kagi/issues/655)）。 |
| 2026-09-10 | 実験 F の mixed-stash は scenario 登録せず、synthetic の pristine copy への決定的導出で作る | `--scenario mixed-stash` は P0 未実装で、allowlist は共有 `fixture.rs` にあるため実験 owner が自分の module だけでは足せない。F は divergence の実験で timing に依存しないので、pristine copy に固定の stash setup を timer 外で適用すれば §4.3 の copy 契約を満たしたまま共有 library を触らずに済む。再現性は seed 627 の manifest と report に逐語記載する setup コマンド列で担保する（[#627](https://github.com/TomiXRM/kagi/issues/627)）。 |
| 2026-09-08 | dirty Pull の衝突予測は `git2::merge_file` で実際に 3-way merge を試す | 「同じパスが両側で変わった」だけでは衝突とは限らない。上流が先頭行、ローカルが末尾行を変えた実測ケースでは自動 merge が成功した。外れる警告はユーザーに警告を読ませなくするため、content merge が判定できない binary・mode/type 変更・片側の追加削除だけを「可能性」として扱う（[#625](https://github.com/TomiXRM/kagi/issues/625)、[ADR-0192](adr/0192-dirty-pull-conflict-preview.md)）。 |
| 2026-09-08 | `gh issue list` は必要件数を覆う `--limit` を明示する | `gh issue list --help` で既定値が 30 と確認された。31 件以上あっても既定実行は警告せず 30 件で打ち切るため、全件を前提にした棚卸しでは件数不足を検知できない。決定日は確認日。 |
| 2026-09-08 | GUI E2E runner は必ず `KAGI_GUI_E2E_ONLY` でシナリオを絞る | 絞らない実行はシナリオごとに native window を開き、約 1,400 窓で macOS の WindowServer を落としてマシンを再起動させた（[#555](https://github.com/TomiXRM/kagi/pull/555)）。`check-e2e-window-helper` は窓の確保経路だけを検査し、実行範囲は制限しない。 |
| 2026-09-07 | Cargo の `target/` は worktree ごとに分離し、共有しない | 2026-09-07 の独立再現で、共有 target 内の `kagi-domain`・`kagi-git`・`kagi` artifact が衝突し、Cargo が stale rlib を `Fresh` と再利用して新規 module に E0432 を出した。clean build が約 3–4 GB/worktree になるディスク費用は、この誤再利用を防ぐため意図的に受け入れる（[#520](https://github.com/TomiXRM/kagi/pull/520)）。 |
| 2026-09-07 | libgit2 の clean 判定を壊す fixture 書き換えは、mtime の秒境界を越すかファイルサイズを変える | libgit2 1.9.4 の index stat cache は、同一秒内に同一サイズで書き換えた working-tree file を clean と報告しうる。blob は変わっていても status が変わらず、#458 の flake になった。fixture の `beta` → `BETA` を `BETA!` にしてサイズを変えると再現しなくなった（[#458](https://github.com/TomiXRM/kagi/issues/458)）。 |
| 2026-09-06 | CI invariant は shell の `grep` ではなく `uv run --project ci` の Python gate で実装する | BSD grep は bounded repetition を 255 で打ち切るため、GNU grep では有効な長いパターンが macOS では何も検査せず成功しうる。実際に #454 で gate が 0 files checked のまま緑になり、人間が発見したため `check-shell-hygiene` を設けた。 |
| 2026-07-21 | cosmic-text 0.19 の Han-unification patch は upstream PR #522 が merge され、GPUI が取り込む release に入るまで維持する | `ja-JP` でも Simplified Chinese glyph が選ばれる fallback path を直す 1 commit patch で、0.14 から 0.19 へ rebase 済み。2026-09-08 時点で [upstream PR #522](https://github.com/pop-os/cosmic-text/pull/522) は open。merge だけで外すと GPUI の選択版には未収録なので、収録 release への更新時に `[patch.crates-io]` を撤去する（[ADR-0130](adr/0130-bundled-japanese-font-fallback.md)）。 |
| 2026-07-14 | GPUI は Zed main を `rev` なしで追い、`runtime_shaders` は `gpui_platform` で有効にする | 2026-07-14 の commit `287b46eb` で crates.io の gpui 0.2.2 から移行した。gpui-component が Zed の default branch を `rev` なしで参照するため、Kagi 側の Zed dependency に `rev` を付けると Cargo では別 source となり、gpui が 2 組入って型が一致しない。実際の Zed pin は `Cargo.lock` に置き、更新時は `cargo update -p gpui` と gpui-component の `rev` を揃えて意図的に上げる。`runtime_shaders` feature は `gpui` ではなく `gpui_platform` 側にある。[ADR-0001](adr/0001-gpui.md) は移行前の構成を記述しており、修正は別 issue で扱う。 |
| 2026-06-12 | gpui 0.2.2 は `runtime_shaders` feature を有効にする | gpui 0.2.2 の `build.rs` は通常 `xcrun metal` で Metal shader を build 時 compile し、Command Line Tools のみの macOS では `metal` が無く失敗する。`runtime_shaders` は compile を実行時へ移し、full Xcode 依存を避ける（[ADR-0001](adr/0001-gpui.md)）。この構成は 2026-07-14 の Zed main 移行で superseded。上の 2026-07-14 の行を参照。 |
