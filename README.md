# Kagi 🔑

コミットグラフ中心の**安全な** Git GUI クライアント。Rust + [GPUI](https://www.gpui.rs/)(Zed の UI フレームワーク)で構築しています。

> 設計思想: 「いま repo がどういう状態か」を常に見せ、**すべての Git 操作の実行前に何が起きるかを表示**し、ローカルリポジトリを壊さないことを最優先にする。詳細は [docs/requirements.md](docs/requirements.md) と [docs/adr/](docs/adr/) を参照。

## 動作環境

- macOS(Apple Silicon / Intel)
- Rust stable(rustup でインストール)
- **full Xcode は不要**(Xcode Command Line Tools のみで可。gpui の `runtime_shaders` feature を使用しているため)
- ネットワーク接続不要(MVP はローカル repo 専用)

## ビルドと起動

```sh
# クローン
git clone https://github.com/TomiXRM/kagi.git
cd kagi

# 起動(開きたい repo のパスを渡す)
cargo run -- /path/to/your/repo
```

- 初回ビルドは gpui / libgit2 のコンパイルで数分かかります(2回目以降は数秒)
- リリースビルドは `cargo run --release -- <repo-path>`
- bare repository は開けません(working tree のある通常の repo を指定してください)

### お試し用の fixture repo

手元の repo を触らずに試したい場合は、付属スクリプトで検証用 repo を生成できます:

```sh
REPO=$(bash scripts/make_fixture.sh)   # 最終行に repo パスが出力される
cargo run -- "$REPO"
```

fixture には分岐・merge・remote(ahead/behind)・tag・stash・dirty working tree が一通り含まれています。

## 配布とインストール

リリースは GitHub Releases から配布します(ADR-0047 / Phase 1)。

| OS | 形態 | 備考 |
|----|------|------|
| macOS(Apple Silicon / Intel)| `Kagi-<version>-<arch>.dmg` | **未署名(ad-hoc 署名)**。初回起動に Gatekeeper 回避が必要(下記)|
| Linux(x86_64)| `kagi-<version>-x86_64.tar.gz`(bin + `.desktop` + icon)| 展開して `bin/kagi` を実行 |

各リリースには `SHA256SUMS-*.txt` を同梱しています。ダウンロード後に整合性を確認してください。

### macOS:未署名アプリの起動(Gatekeeper 回避)

Kagi はまだ Apple Developer ID による署名・notarization を行っていません(ad-hoc 署名のみ)。
そのため初回起動時に Gatekeeper が「開発元を確認できません」と警告します。以下のいずれかで回避できます:

- **Finder で右クリック → 開く**(初回のみ。2回目以降は通常どおりダブルクリックで起動)
- もしくは quarantine 属性を外す:

  ```sh
  xattr -dr com.apple.quarantine /Applications/Kagi.app
  ```

Apple Developer Program の取得後に署名 + notarization(ADR-0038 Phase 2)へ移行する予定です。

### ソースからバンドルを作る(開発者向け)

`xtask` で `.app` / `.dmg` / Linux tar.gz をローカル生成できます(macOS 標準ツールのみ・外部依存なし):

```sh
bash scripts/make_icon.sh                 # assets/icon/(AppIcon.icns + Linux PNG)を生成
cargo run -p xtask -- bundle-macos        # target/dist/Kagi.app(ad-hoc 署名済)
cargo run -p xtask -- dmg-macos           # target/dist/Kagi-<version>-<arch>.dmg
cargo run -p xtask -- bundle-linux        # target/dist/kagi-<version>-x86_64.tar.gz(レイアウト検証)
```

## 画面と操作

```
┌────────────────────────────────────────────────────────────┐
│ Header: repo名 · HEAD · status概要 · commit数   [Stash]     │
├──────────┬──────────────────────────────┬──────────────────┤
│ Sidebar  │ Commit Graph                 │ Detail Panel      │
│ BRANCHES │  グラフ + SHA + message       │ (commit選択時)     │
│ STASHES  │  + refバッジ + author + date  │  metadata          │
│          │                              │  changed files tree│
│          │                              │  → file diff       │
└──────────┴──────────────────────────────┴──────────────────┘
```

| 操作 | 結果 |
|------|------|
| commit 行をクリック | 右に Detail Panel(SHA / author / parents / message / changed files) |
| changed files のファイルをクリック | unified diff 表示(`← back` で戻る) |
| Sidebar の branch をクリック | **checkout の plan モーダル**(下記) |
| Detail Panel の `+ Create branch here` | その commit を起点に branch 作成(名前入力 + live 検証) |
| Detail Panel の `Cherry-pick onto <branch>` | **dry-run preview 付き** cherry-pick(下記) |
| Header の `Stash`(dirty 時のみ) | stash push(untracked 込み) |
| Sidebar の stash をクリック | stash apply(clean 時のみ。stash は消えません) |

## 安全設計(このアプリの核)

すべての書き込み操作は **plan → 確認 → preflight → execute → verify** のパイプラインを通ります:

- **plan モーダル**: 実行前に「現在の状態 → 実行後の状態」「警告(黄)」「実行できない理由 = blocker(赤)」「失敗時の復旧手順」を表示。blocker がある場合、実行ボタン自体が出ません
- **cherry-pick の dry-run preview**: libgit2 の in-memory merge で **working tree に一切触れずに** conflict の有無と変更ファイルを予測。conflict が予測される cherry-pick は実行できません
- **実装されていない危険操作**: `force push` / `reset --hard` / `git clean` / `stash drop` はコードベースに存在しません
- checkout は safe モードのみ。変更を失う可能性がある場合は blocker(stash を提案)

## 開発者向け

```sh
cargo test          # 全テスト(domain / backend / graph layout / 操作パイプライン)
cargo check         # 型チェック
```

- 設計ドキュメント: [docs/requirements.md](docs/requirements.md) / [docs/architecture.md](docs/architecture.md) / [docs/adr/](docs/adr/)
- 開発チケット: [docs/tickets/INDEX.md](docs/tickets/INDEX.md)(1チケット = 1機能で進行)
- テスト・動作検証は必ず fixture / tempdir の repo に対して行うこと(既存 repo への書き込み禁止)

### 検証用の環境変数(開発専用)

ヘッドレス検証用。**通常利用では設定しないでください**(`KAGI_AUTO_CONFIRM` は確認ダイアログをスキップします):

| 変数 | 効果 |
|------|------|
| `KAGI_SELECT_FIRST=1` | 起動時に先頭 commit を自動選択 |
| `KAGI_OPEN_FIRST_FILE=1` | ↑と併用で最初の changed file の diff を開く |
| `KAGI_PLAN_CHECKOUT=<branch>` ほか | 各操作の plan を自動生成(詳細は docs/tickets/T013〜T016) |
| `KAGI_AUTO_CONFIRM=1` | blocker がなければ plan を自動実行(**fixture 専用**) |

## ステータス

MVP 開発中。完了済み: repo 表示(graph / branch / tag / stash / status)、commit 詳細 + ファイルツリー + diff、checkout / branch 作成 / stash push・apply / cherry-pick(すべて plan 確認付き)。進行中: UI 磨き込みと operation log。ロードマップは [docs/requirements.md](docs/requirements.md) §2。
