# ADR-0220: 自作テーマの JSON 形式と実行時テーマ一覧（#922）

- Status: **Accepted**
- Date: 2026-10-03
- Related: [ADR-0036](0036-color-themes.md)、[ADR-0091](0091-issue-13-structural-refactor.md)、[ADR-0121](0121-zed-informed-modularization.md)、#921、#922

## 文脈

従来の `kagi-ui-core::theme::THEMES` はコンパイル時に定義したテーマだけを持ち、`theme() -> &'static Theme` と組み込みテーマへの atomic index で全画面から参照している。#921 で Settings と command palette は名前順の `themes_by_name()` を使うようになったが、macOS / Linux のメニューは固定の theme command 7 件だけを列挙している。自作テーマを別のテーマ管理経路へ足すと、表示順、切替、`settings.json` の slug 復元、terminal と gpui-component の配色がずれる。

## 決定

1. **形式と配置。** `$KAGI_LOG_DIR/themes/*.json`、指定がなければ `~/.kagi/themes/*.json` を起動時と command palette の「テーマを再読み込み」で読む。監視はしない。単一 JSON object の `slug`（安定した識別子）と `name`（表示名）は必須。ほかのキーは `Theme` のフィールド名をそのまま使い、`syntax` は十個のフィールドを持つ入れ子 object とする。`"extends": "<slug>"` は**組み込みテーマだけ**を指定でき、残りのフィールドはそのテーマから継承し、記載したキーだけを上書きする。`extends` を省く場合は `slug`・`name` を含む `Theme` の**全フィールド**を要求する。標準の RGB 色は `"#rrggbb"`（6 桁、`#` 必須）、`term_*` の RGB も同形。`dark` は JSON boolean。`lane_hsl` は八つの `[h, s, l]`（各 0..1）の配列、`avatar_sat` / `avatar_light` は 0..1 の数値、RGBA である `term_selection` だけは `{"color":"#rrggbb","alpha":0..255}`。`syntax` の各色も `"#rrggbb"`。VS Code 形式や変換機能は追加しない。
2. **検証と障害隔離。** 未知のキー、フィールド欠落、型・色形式・配列長・値域の誤り、組み込み slug との衝突をファイル単位で拒否する。同じ slug の自作ファイルが複数ある場合はファイル名順で最初だけ採用する。拒否理由にはファイル名を付け、診断ログと UI 準備後の bounded toast に知らせる。残りのファイルとアプリ起動を妨げない。削除された自作テーマを選択中に再読込した場合は、その実行中だけ既定 `THEMES[0]` へ戻す。保存済み slug は上書きせず、ファイルを戻した再起動で選び直せる。
3. **単一のテーマ一覧と寿命。** `THEMES[0]`（Catppuccin Mocha）は変えず、組み込み一覧に検証済み自作テーマを追加した実行時の一覧を `kagi-ui-core::theme` が所有する。古い render・Select・terminal の参照が再読込と競合しても、所有された不変スナップショットが寿命を保証し、使われなくなれば解放する。`Box::leak` で再読込のたびにメモリを残さない。組み込みテーマの参照は既存の高速 path を保つ。`themes_by_name()` の表示名（ローカライズに依存しない）の大文字小文字を区別しない順を Settings / command palette / macOS menu / Linux menu で共有する。固定の七件だけのメニュー列挙を実行時一覧へ切り替え、既存のキー割り当ては維持する。
4. **選択と公開ドキュメント。** 既存の `set_active` / `KagiApp::set_theme` に合流し、`settings.json` には slug の文字列だけを保存する。再起動時の `KAGI_THEME` 優先と既存の `[kagi] theme: … dark=…` 行は維持する。現在の terminal の配色、gpui-component のテーマ橋、`accent_text_on` の可読性補正を自作テーマにも適用する。`docs/themes.md` に最小の継承例・全フィールド例と**実際の呼び出し箇所を調べた**全トークンの用途・値を載せ、README と Settings からリンクする。`Theme` のシリアライズから得たフィールド集合（`syntax.*` を含む）と表の行を比較するテストで追加漏れを防ぐ。
5. **Settings のフォルダー操作。** Appearance の自作テーマ欄は `themes_dir()` が解決したパスを選択・コピー可能な文字列で示し、「テーマフォルダーを開く」は欠けている空ディレクトリだけを作成してから OS のファイルマネージャー(macOS は `open`、Windows は `explorer`、その他は `xdg-open`)へ渡す。新しい JSON や `settings.json` は書かない。作成・起動に失敗したときはローカライズした bounded toast へ知らせ、I/O は UI thread の外で行う。「テーマを再読み込み」ボタンは command palette と同じ `reload_themes` を呼び、二つ目の再読込経路を作らない。
6. **再読み込みのスレッドと順序。** `read_custom_themes` は I/O と JSON 解析だけを行い、窓が存在する間の Settings / palette の再読み込みでは `background_spawn` で実行する。完了後に foreground で最新のリクエスト世代だけが registry・menu・Select・terminal / gpui-component の色と toast を一緒に更新し、先に要求された結果は破棄する。起動時だけは窓作成前に同じ読み込み・検証関数を同期実行し、保存済み slug / `KAGI_THEME` を最初の描画前に解決する。起動時の同期処理は開いた窓を止めない。
7. **列挙できないフォルダーは空ではない。** フォルダーが無いことは「自作テーマなし」だが、権限や一時的な mount 障害でフォルダーを列挙できないことは別の状態として `read_custom_themes` が `Err` を返す。再読み込みはそのとき registry を差し替えず(読み込み済みの自作テーマと選択中のテーマを残す)、理由を bounded toast に出す(#930 review)。

## 却下した案

- **VS Code テーマの直接インポート。** この公開形式と画面・トークンの対応表があれば利用者が変換でき、別形式の互換性維持を負わずに済む。
- **自作テーマの値を新しい UI 状態や設定ファイルに複製する。** `theme()` と既存の settings store を迂回してテーマ切替の経路を増やすため却下。
- **動的テーマを `'static` に leak する。** 再読込するたび古い世代の色や表示名がプロセス終了まで残るため却下。

## 検証

継承あり・なし、永続 slug 復元、壊れた JSON の隔離と競合、再読込の置換・削除時 fallback、全トークンの文書網羅を unit で確認する。Tier A は Settings / palette / メニューへの一覧追加と実際の切替に加え、Settings のボタンで空フォルダーの作成・再読込・作成失敗 toast を確認する。Tier B は実際に自作テーマが Settings の一覧とフォルダー操作に出ている画面を撮影し、画像は PR のコード履歴とは別の `pr-assets/<topic>` に保存する。
