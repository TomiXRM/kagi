# W4-TABS: リポジトリ tab 切り替え + ディレクトリ選択(ユーザー要望)

- Status: in-progress
- 担当: worktree agent(Opus)
- 関連 ADR: 0027(tabs)/ 0028(picker・welcome)

## 背景

現在 repo 選択は起動引数のみ。GUI からディレクトリを選んで開きたい。
複数 repo を上部 tab で切り替えたい(GitKraken 同様)。

## スコープ

1. **tab モデル**(ADR-0027): `RepoTab { path, name }` + `tabs: Vec<RepoTab>` + `active_tab: usize`。
   CLI 引数 = 初期 tab。`switch_repo(index)` は snapshot 再構築 + per-repo UI 状態リセット
   (selection / diff_cache / main_diff / modals / commit_panel)。同一 repo は既存 tab に switch
2. **tab strip UI**: ヘッダツールバーの**上**。repo 名(truncate + フルパス tooltip)/
   active 強調 / × close / 右端 [+]。tab 数が多い時は最小幅 + truncate(横スクロールは later)
3. **picker**(ADR-0028): `cx.prompt_for_paths(files:false, directories:true, multiple:false)`。
   失敗(非 repo)は tab を作らず error toast + footer
4. **Welcome 画面**: tabs が 0 のとき中央に「Open Repository…」ボタン。
   従来の usage エラー画面を置き換え(エラー文字列は headless 互換のため維持)
5. **watcher の再 arm**(ADR-0027): `watcher_generation` 方式。run_app 固定 spawn を
   `arm_watcher` メソッド化し、switch/open/close で旧 loop を自然終了させる
6. **terminal session**: `HashMap<PathBuf, KagiTerminalSession>` で tab 横断保持
   (active repo の session を bottom panel に表示、lazy 生成は既存どおり)
7. **headless**: `KAGI_OPEN_REPO=<path>` で tab 追加 + switch。
   `[kagi] tabs: n=<N> active=<i> <name>` ログ。既存の引数1つ起動の全ログに回帰なし

## 完了条件

- [ ] 引数なし起動 → Welcome → picker で repo を開ける(PM 実操作確認)
- [ ] [+] で2つ目の repo を開き、tab クリックで切り替え(graph/sidebar/statusbar が全て切り替わる)
- [ ] × で tab close、最後の close で Welcome に戻る
- [ ] 切り替え後に watcher が新 repo を監視している(fixture でターミナル commit → 自動 refresh)
- [ ] 非 repo ディレクトリ選択で error toast、tab 増えない
- [ ] `KAGI_OPEN_REPO` headless 検証 + tabs ログ
- [ ] `cargo test` 全パス + own-code warning 0、既存 headless 検証に回帰なし
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/tabs.rs`(新規、できる限りここに集約)
- `src/ui/mod.rs` / `src/ui/watcher.rs` / `src/ui/terminal.rs`(最小限)
- `src/main.rs`(引数 → 初期 tab、KAGI_OPEN_REPO)
- `docs/tickets/W4-TABS.md`

## 触ってはいけないファイル

- `src/git/` / `src/graph/` / `tests/*` / `scripts/*` / `Cargo.toml` / 他 docs

## テスト方法

1. `cargo test`(exit code 直接確認)
2. fixture を2つ作って(make_fixture.sh ×2)tab 切り替えを実機 + headless で検証
3. 検証は fixture / tempdir のみ

## リスク

- **並行 lane 注意**: codex-cm-base lane が src/ui/mod.rs を同時に編集中。
  mod.rs の変更は最小限にし、**変更点を完了報告で全列挙**(PM が merge する)
- switch 時の状態リセット漏れ(stale diff/modal)— リセット対象をチケットの列挙どおりに
- watcher の旧 loop が残ると二重 reload になる。generation 検査を update 毎に行う
- 文字列切り詰めは chars() ベース / force 系コード追加禁止(全体規約)
