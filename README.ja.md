<div align="center">

<img src="assets/icon/icon_256x256.png" width="120" alt="Kagi icon" />

# Kagi 🔑

### 何が起きるかを必ず見せる。そしてリポジトリを壊せない Git GUI。

[![Release](https://img.shields.io/github/v/release/TomiXRM/kagi?include_prereleases)](https://github.com/TomiXRM/kagi/releases)
[![Stars](https://img.shields.io/github/stars/TomiXRM/kagi?style=flat)](https://github.com/TomiXRM/kagi/stargazers)
[![Downloads](https://img.shields.io/github/downloads/TomiXRM/kagi/total)](https://github.com/TomiXRM/kagi/releases)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)

Rust + [GPUI](https://www.gpui.rs/)([Zed](https://zed.dev/) の UI フレームワーク)製。

**[⬇ ダウンロード(macOS · Linux · Windows)](https://github.com/TomiXRM/kagi/releases/latest)** &nbsp;·&nbsp; [English README](./README.md)

<img src="docs/images/hero.png" width="900" alt="Zed リポジトリを開いた Kagi — コミットグラフのレーン、ref バッジ、HEAD リング、ブランチツリー" />

</div>

---

Kagi は「**Git コマンドで不意打ちを食らわない**」ことを軸に作られたデスクトップ Git クライアントです。書き込みの前に、現在の状態・実行後の予測・警告や blocker・取り消し方法を必ず提示し、`plan → confirm → preflight → execute → verify` のパイプラインを通して実行します。

作業を失わせるコマンド — `push --force` / `reset --hard` / `git clean` — は、確認ダイアログで止めているのではありません。**そもそもコードベースに存在しません。**

> **ステータス** — 活発に開発中で、macOS では日常的に使用しています。Linux 対応。**Windows は実験的**(CI ビルドのみ・メンテナ未検証)。macOS ビルドは ad-hoc 署名のみ(notarize 未対応)— 初回起動の手順は[インストール](#-インストール)を参照。

## 安全性こそがプロダクト

<div align="center">
<img src="docs/images/safety-plan.png" width="880" alt="Push の plan モーダル — 現在 → 予測、non-fast-forward の警告、push される commit、復旧手順。実行前にすべて提示される" />
</div>

すべての書き込みは、まず **plan** を開きます — 現在 → 予測の状態、警告、blocker、そして平易な復旧手順。blocker があるときは、押せる実行ボタンがそもそも描画されません。後付けの「本当に実行しますか?」ダイアログではなく、操作が走る唯一の経路がこれです。

| Kagi が保証すること | どう担保しているか |
|---|---|
| **結果を先に見せる** | すべての操作で plan(現在 → 予測、警告、blocker、復旧手順)を提示。blocker があれば実行ボタンは描画すらされない。 |
| **破壊的コマンドが存在しない** | `push --force` / `reset --hard` / `git clean` は**どこにも実装されていない** — 規律ではなく CI の grep ゲートで担保。 |
| **conflict は遭遇ではなく予測** | cherry-pick / revert / merge / checkout の conflict を `libgit2` の in-memory dry-run で検出。予測時点で working tree には触れない。 |
| **conflict 解決はやり直せる** | Conflict Mode はいつでも操作前の状態へ abort 可能。解決途中の内容は自動保存。 |
| **黙って失われない** | checkout 前の自動 stash、discard 前の ODB blob バックアップ、before/after を記録する追記専用の操作ログ(`~/.kagi/operations.jsonl`)。 |
| **ref の移動は最後** | working tree を先に書き、ref は最後に動かす。操作の途中で失敗しても HEAD は元のまま。 |

## conflict を 1 行ずつ解決

<div align="center">
<img src="docs/images/conflict-blink.png" width="900" alt="Arduino Blink スケッチの Conflict Mode — Current / Result / Incoming の 3 ペイン、file・chunk・行単位の採用トグル、リアルタイム Result プレビュー、conflict ダッシュボード" />
</div>

merge / rebase / cherry-pick / revert が conflict すると、Kagi は **Conflict Mode** に入ります。**Current**・リアルタイム編集できる **Result**・**Incoming** の 3 ペインエディタで、file / chunk / **行単位**の採用トグル、conflict ダッシュボード、Save → stage → Continue フローを提供します。

上のスクリーンショットでは、片方の branch が LED をボード内蔵ピンに変更し、もう片方が点滅を速くしたので、**LED のピン**と **`delay()` の間隔**の両方が conflict しています。同じファイルの中で **ピンは一方の branch から、間隔はもう一方から**採用し、クリックするたびに Result が更新される様子を確認できます。あるいは abort すれば、開始前の状態に正確に戻ります。

## 読めるコミットグラフ

<div align="center">
<img src="docs/images/graph-stash.png" width="900" alt="色分けレーン・ref バッジ・HEAD リング・先頭の WIP 行、そして base commit へ黄色い線で繋がる stash を描いたコミットグラフ" />
</div>

色分けされたレーンが各 branch の履歴を辿り、ref バッジと HEAD リングが現在地を示し、merge ノードがインラインで描かれ、先頭には WIP 行が常駐します。さらに **stash もグラフの中に描画**され、作成元の commit へ線で繋がります。ラベル → ノードのコネクタが各 branch / tag をその commit に結びます。仮想化されているので 1 万 commit 超でも軽快です(スクリーンショットは実際の履歴 — 上は Zed、ここは小さな fixture)。

## 日常使いの残り

- **コミットスイート** — `+N −M` の diffstat バー付きステージング、pre-commit チェックリスト(conflict marker / secret / 巨大バイナリ)、branch ごとの draft 自動保存、`type(scope): summary` メッセージテンプレート、SHA 変化を見せる amend。
- **Smart commit message** — rule-based 生成は常時利用可。**ローカル Ollama LLM は明示 opt-in**(staged diff のみ・localhost のみ・初回同意)。
- **全部非同期** — checkout / commit / stash / pull / push / merge … は UI スレッド外で実行し、回転する sync アイコンのスナックバーを表示。ウィンドウは固まりません。
- **居心地よく** — 6 つのカラーテーマ、英語 / 日本語 UI(Git のドメイン語はどちらでも英語のまま)、内蔵ターミナル、リポジトリタブ、branch プレフィックスのツリーサイドバー、操作ログ、UI 全体の均一ズーム。

## 📦 インストール

[**GitHub Releases**](https://github.com/TomiXRM/kagi/releases) から最新ビルドを入手してください。各リリースには `SHA256SUMS-*.txt` が付属します — ダウンロードを検証してください。v0.3.4 以降はアプリ内から更新の確認・インストールもできます。

| OS | アセット |
|----|---------|
| macOS (Apple Silicon) | `Kagi-<version>-arm64.dmg` |
| Linux (x86_64 / arm64) | `kagi-<version>-<arch>.tar.gz`(バイナリ + `.desktop` + アイコン)、または AppImage zip `kagi_Linux-AppImage_<arch>.zip` |
| Windows (x86_64) | `kagi-<version>-x86_64-windows.zip` — 展開して `kagi.exe` を実行(自己完結) |

<details>
<summary><b>macOS — 未署名ビルドの初回起動</b></summary>

Kagi はまだ **Apple の notarize に未対応**(ad-hoc 署名のみ・Apple Developer ID 未取得)のため、Gatekeeper が「開発元を確認できない」と警告します。いずれかで:

1. **`Kagi.app` を右クリック → 開く → 開く**(初回のみ。以降は通常起動)、または
2. quarantine 属性を外す:
   ```sh
   xattr -dr com.apple.quarantine /Applications/Kagi.app
   ```

署名 + notarize は Apple Developer Program 加入後に対応予定です。
</details>

<details>
<summary><b>Linux — AppImage</b></summary>

```sh
unzip kagi_Linux-AppImage_<arch>.zip && bash install_linux_desktop.sh
```
で `~/.local` 配下に登録されます(アイコン + `.desktop`、完全オフライン)。
</details>

<details>
<summary><b>Windows — 初回起動とステータス</b></summary>

Windows ビルドは**実験的 / ベストエフォート**(CI でビルド・パッケージ。メンテナによる実機検証は未了 — 不具合は報告歓迎)。未署名のため SmartScreen が初回に警告します:**詳細情報 → 実行**。`PATH` に通常の `git` があることを推奨します(Kagi は `git` を呼び出し、内蔵ターミナルを開きます)。
</details>

## 🛠️ ソースからビルド

<details>
<summary><b>必要要件と手順</b></summary>

Rust stable(rustup)に加えて:

- **macOS** — **Xcode Command Line Tools のみ**(フル Xcode 不要。Kagi は GPUI の `runtime_shaders` を使用)。
- **Linux** — GPUI のネイティブビルド依存。Debian/Ubuntu:
  ```sh
  sudo apt-get install -y \
    libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libx11-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
    libasound2-dev libvulkan-dev libzstd-dev
  ```

```sh
git clone https://github.com/TomiXRM/kagi.git
cd kagi
cargo run --release -- /path/to/your/repo
```

初回ビルドは数分(gpui / libgit2)、以降は数秒です。bare リポジトリは非対応(working tree のある通常のリポジトリを指定してください)。

**`kagi` コマンドを `PATH` に入れる:**
```sh
cargo install --path .          # ~/.cargo/bin に `kagi` を導入
kagi /path/to/your/repo         # そのリポジトリで Kagi を開く
kagi                            # 引数なし → Welcome 画面
```
バイナリはアセットを埋め込むため自己完結です。

**自分のリポジトリに触れず試す:**
```sh
REPO=$(bash scripts/make_fixture.sh)   # branch・merge・remote・tag・stash・dirty な working tree
cargo run -- "$REPO"
```
</details>

## 🧑‍💻 開発

<details>
<summary><b>テスト・ドキュメント・v1.0 再アーキテクチャ</b></summary>

```sh
cargo test --workspace
```

- 設計ドキュメント: [docs/requirements.md](docs/requirements.md) · [docs/architecture.md](docs/architecture.md) · [ADR](docs/adr/)
- **実リポジトリでテストしないこと** — `scripts/make_fixture.sh` / tempdir を使用。`KAGI_*` 環境変数は headless テスト専用です。
- Kagi は、安全第一の設計を規約ではなく型システムで担保するため、レイヤ化された Cargo workspace へ再アーキテクチャ中です。詳細は [docs/rearch/](docs/rearch/) と [ADR 0072 以降](docs/adr/)。中核の不変条件: UI は `git2` に直接触れず、すべての Git 操作は `plan → confirm → preflight → execute → verify → log` パイプラインを通る(CI の grep ゲートで担保)。
</details>

## 📄 ライセンス

[MIT](LICENSE)。同梱のターミナルコンポーネント(`vendor/gpui-terminal`)は上流で MIT OR Apache-2.0 ライセンスであり、ここでは MIT として利用しています。
