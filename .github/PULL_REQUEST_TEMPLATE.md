<!--
統合する側はこの 4 節を上から順に読む。節の順序は固定。

Verified — 何を走らせたか — は既に多くの PR が書いている。書かれないのは
その逆で、走らせなかったものである。#626 はゲート 21 本とテスト 115 スイートが
全て緑、GUI E2E も通っていて、実機では Pull ボタンが完全に無反応だった。
最後の節はそのための欄なので、空欄のまま出さない。

このテンプレートはゲートではない。埋まっていない PR を機械的に落とすことはしない。
-->

## What changed

<!--
なぜ変えたか、から書く。何を変えたかは diff とコミットが既に言っている。
- 直した問題、または足りなかった機能を 1〜3 行で
- 設計判断があるなら、採らなかった案とその理由も一行で
-->

## Docs

<!--
触った文書を「節」の単位で書く。ADR / docs/rearch / docs/tickets / CHANGELOG。
- 例: ADR-0192 に §2c を追加 / CHANGELOG の [Unreleased] - Fixed に 1 行
- 検証の seam (環境変数, scripts/*, runner の scenario) を触ったなら verify skill も
  更新し、`skill 更新済み` と書く
- 無いなら「なし」と、その理由 (例: 挙動を変えていないため)
-->

## Verified

<!--
✓ だけでは証拠にならない。各行に走らせたコマンドと結果の数字を書く。
走らせていない行は消さず、「未実行」と書いて残す。消すと読む側から見えなくなる。
-->

- [ ] `cargo fmt --all --check` / `cargo clippy --workspace` / `KAGI_LOG_DIR=$(mktemp -d) cargo test -j 8 --workspace` (N tests, 0 failed)
- [ ] `uv run --project ci check-all`
- [ ] fixture / headless: <走らせたコマンドと、確認した `[kagi]` 行>
- [ ] Tier A (GUI E2E runner): `KAGI_GUI_E2E_ONLY=<substr>` (<scenario> PASS)
- [ ] Tier B (実機 GUI / pidclick): <どのウィンドウで何を押し、何が見えたか>

## Not verified — needs a human

<!--
触れなかった経路をここに書く。merge する側はこの節だけを読んで、自分の手で
何を確かめるかを決める。空欄にしない。

書くもの:
- 変えたのに実機で開いていない画面、押していないボタン
- 自動テストで再現できなかった条件 (watcher reload との競合、フォーカス、IME、
  メニュー経由の操作、ネットワーク、権限ダイアログなど)
- 代わりに何で担保したか (Tier A のみ / 単体テストのみ / 未担保)

本当に無いなら「なし」と、なぜ無いのかを書く
(例: .github/ 配下のみの変更で、実行されるコード経路が無いため)。
-->
