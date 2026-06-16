<div align="center">

<img src="assets/icon/icon_256x256.png" width="120" alt="Kagi icon" />

# Kagi 🔑

### 何が起きるかを必ず見せる。そして、リポジトリを壊しようがない Git GUI。

[![Release](https://img.shields.io/github/v/release/TomiXRM/kagi?include_prereleases)](https://github.com/TomiXRM/kagi/releases)
[![Stars](https://img.shields.io/github/stars/TomiXRM/kagi?style=flat)](https://github.com/TomiXRM/kagi/stargazers)
[![Downloads](https://img.shields.io/github/downloads/TomiXRM/kagi/total)](https://github.com/TomiXRM/kagi/releases)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)

Rust + [GPUI](https://www.gpui.rs/)([Zed](https://zed.dev/) の UI フレームワーク)製。

**[⬇ ダウンロード(macOS · Linux · Windows)](https://github.com/TomiXRM/kagi/releases/latest)** &nbsp;·&nbsp; [English README](./README.md)

<img src="docs/images/hero.png" width="900" alt="Zed リポジトリを開いた Kagi — 色分けレーンのコミットグラフ、ref バッジ、HEAD リング、branch ツリー" />

</div>

---

Kagi は、たったひとつの考えを軸に作られたデスクトップ Git クライアントです — **Git コマンドで不意打ちを食らうことがあってはならない。** 何かを書き込む前に、現在の状態・実行後に予測される状態・警告や blocker・取り消す方法を必ず示し、そのうえで `plan → confirm → preflight → execute → verify` のパイプラインを通して操作を実行します。

作業を失わせるコマンド — `push --force` / `reset --hard` / `git clean` — を確認ダイアログで止めているわけではありません。**そもそもコードベースのどこにもありません。**

> **開発状況** — 活発に開発中で、macOS では日常的に使っています。Linux は対応済み。**Windows は実験的**(CI でビルドするのみ・メンテナ未検証)。macOS ビルドは ad-hoc 署名のみで notarize は未対応 — 初回起動の手順は[インストール](#-インストール)を参照してください。

## 安全性こそがプロダクト

<div align="center">
<img src="docs/images/safety-plan.png" width="880" alt="push の plan モーダル — 現在 → 予測の状態、non-fast-forward の警告、push される commit、復旧手順。実行前にこれらすべてを提示する" />
</div>

書き込み操作はすべて、まず **plan** を開きます — 現在 → 予測の状態、警告、blocker、そして平易な言葉で書かれた復旧手順。blocker があるあいだは、押せる実行ボタンが存在しません。これは後付けの「本当に実行しますか?」ダイアログではなく、操作を実行する唯一の経路です。

| Kagi が約束すること | どう担保しているか |
|---|---|
| **結果を先に見せる** | すべての操作で plan(現在 → 予測の状態、警告、blocker、復旧手順)を提示。blocker があれば実行ボタンは描画すらされない。 |
| **破壊的コマンドが存在しない** | `push --force` / `reset --hard` / `git clean` は**どこにも実装されていない** — 規律ではなく CI の grep ゲートで担保している。 |
| **conflict は遭遇するものではなく予測するもの** | cherry-pick / revert / merge / checkout の conflict を `libgit2` の in-memory dry-run で検出。予測の時点では working tree に一切触れない。 |
| **conflict 解決はやり直せる** | Conflict Mode はいつでも操作前の状態へ abort できる。解決の途中経過は自動保存される。 |
| **黙って失われない** | checkout 前の自動 stash、discard 前の object DB への blob バックアップ、before/after を記録する追記専用の操作ログ(`~/.kagi/operations.jsonl`)。 |
| **ref の移動は最後** | working tree を先に書き、ref は最後に動かす。操作の途中で失敗しても HEAD は元の位置のまま。 |

## conflict を 1 行ずつ解決

<div align="center">
<img src="docs/images/conflict-blink.png" width="900" alt="Arduino Blink スケッチの Conflict Mode — Current / Result / Incoming の 3 ペインエディタ、file・chunk・行単位の採用トグル、Result のライブプレビュー、conflict ダッシュボード" />
</div>

merge / rebase / cherry-pick / revert で conflict が起きると、Kagi は **Conflict Mode** に切り替わります。**Current**・その場で編集できる **Result**・**Incoming** の 3 ペインエディタを備え、採用するかどうかを file / chunk / **行単位**で切り替えられるほか、conflict ダッシュボードと Save → stage → Continue のフローを用意しています。

上のスクリーンショットでは、片方の branch が LED をボード内蔵ピンに変え、もう片方が点滅を速くしたため、**LED のピン**と **`delay()` の間隔**の両方が conflict しています。同じファイルの中で **ピンは一方の branch から、間隔はもう一方から**採り、クリックするたびに Result が更新されていく様子がわかります。あるいは abort すれば、始める前の状態にそっくり戻ります。

## 読めるコミットグラフ

<div align="center">
<img src="docs/images/graph-stash.png" width="900" alt="色分けレーン・ref バッジ・HEAD リング・先頭の WIP 行、そして base commit へ黄色い線で繋がる stash を描いたコミットグラフ" />
</div>

色分けされたレーンが各 branch の履歴をたどり、ref バッジと HEAD リングが現在地を示します。merge ノードはインラインで描かれ、先頭には常に WIP 行が並びます。さらに **stash もグラフの中に描画され**、それぞれが作成元の commit へ線で繋がります。ラベルとノードを結ぶコネクタが、すべての branch と tag をその commit に対応づけます。仮想化しているので、1 万 commit を超えるリポジトリでも動きは滑らかです(スクリーンショットはいずれも実際の履歴 — 上は Zed、こちらは小さな fixture)。

## どの commit でも詳しく見る

<div align="center">
<img src="docs/images/diff.png" width="900" alt="commit インスペクタ — author とメッセージ、ファイルごとの +N −M diffstat バーが付いた変更ファイルツリー、シンタックスハイライトと行番号付きの diff" />
</div>

commit を選ぶとインスペクタが開きます。author・co-author・メッセージ全文に加え、ファイルごとに `+N −M` の diffstat バーが付いた変更ファイルツリー、そして `+`/`−` の hunk をシンタックスハイライトと行番号付きで表示する diff が並びます。ファイルを選べば、その diff へ直接ジャンプします。

## 普段使いを支える、その他の機能

- **commit まわり一式** — `+N −M` の diffstat バーが付いた staging、コミット前チェックリスト(conflict marker / secret / 巨大バイナリ)、branch ごとの下書き自動保存、`type(scope): summary` のメッセージテンプレート、SHA の変化を見せる amend。
- **スマートな commit メッセージ** — ルールベースの生成は常に使えます。**ローカルの Ollama LLM は明示的な opt-in のみ**(対象は staged diff だけ・localhost のみ・利用には事前同意が必要)。
- **すべて非同期** — checkout / commit / stash / pull / push / merge … はどれも UI スレッドの外で実行され、処理中は回転するスナックバーが出ます。ウィンドウが固まることはありません。
- **自分好みに** — 6 種類のカラーテーマ、英語 / 日本語の UI(Git のドメイン語はどちらでも英語のまま)、内蔵ターミナル、リポジトリのタブ、branch のプレフィックスでまとめるツリーサイドバー、操作ログ、UI 全体を一括で変えるズーム。

## 📦 インストール

最新ビルドは [**GitHub Releases**](https://github.com/TomiXRM/kagi/releases) から入手できます。各リリースには `SHA256SUMS-*.txt` が付属するので、ダウンロードしたファイルを検証してください。v0.3.4 以降は、アプリ内から更新の確認とインストールもできます。

| OS | アセット |
|----|---------|
| macOS (Apple Silicon) | `Kagi-<version>-arm64.dmg` |
| Linux (x86_64 / arm64) | `kagi-<version>-<arch>.tar.gz`(バイナリ + `.desktop` + アイコン)、または AppImage の zip `kagi_Linux-AppImage_<arch>.zip` |
| Windows (x86_64) | `kagi-<version>-x86_64-windows.zip` — 展開して `kagi.exe` を実行(単体で動作) |

<details>
<summary><b>macOS — 未署名ビルドの初回起動</b></summary>

Kagi はまだ **Apple の notarize に対応していない**(ad-hoc 署名のみで Apple Developer ID も未取得)ため、Gatekeeper が「開発元を確認できない」と警告します。次のいずれかで起動してください。

1. **`Kagi.app` を右クリック → 開く → 開く**(初回だけ。以降は通常どおり起動できます)、または
2. quarantine 属性を外す。
   ```sh
   xattr -dr com.apple.quarantine /Applications/Kagi.app
   ```

署名と notarize は、Apple Developer Program への加入後に対応する予定です。
</details>

<details>
<summary><b>Linux — AppImage</b></summary>

```sh
unzip kagi_Linux-AppImage_<arch>.zip && bash install_linux_desktop.sh
```
これで `~/.local` 配下に登録されます(アイコンと `.desktop` エントリ、完全オフライン)。
</details>

<details>
<summary><b>Windows — 初回起動と現状</b></summary>

Windows ビルドは**実験的かつベストエフォート**です(CI でのビルドとパッケージングまでで、メンテナによる実機検証はまだ済んでいません — 不具合の報告は歓迎します)。未署名のため、初回起動時に SmartScreen が警告します。**詳細情報 → 実行**を選んでください。`PATH` に通常の `git` を通しておくことをおすすめします(Kagi は `git` を呼び出し、内蔵ターミナルを開きます)。
</details>

## 🛠️ ソースからビルド

<details>
<summary><b>必要なものと手順</b></summary>

Rust stable(rustup)に加えて、次が必要です。

- **macOS** — **Xcode Command Line Tools だけ**(フルの Xcode は不要。Kagi は GPUI の `runtime_shaders` を使います)。
- **Linux** — GPUI のネイティブビルド依存。Debian/Ubuntu では次のとおりです。
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

初回ビルドは数分かかりますが(gpui / libgit2)、それ以降は数秒で済みます。bare リポジトリには対応していません(working tree のある通常のリポジトリを指定してください)。

**`kagi` コマンドを `PATH` に通す:**
```sh
cargo install --path .          # ~/.cargo/bin に `kagi` をインストール
kagi /path/to/your/repo         # そのリポジトリで Kagi を開く
kagi                            # 引数なし → Welcome 画面
```
バイナリはアセットをすべて埋め込んでいるので、単体で動作します。

**自分のリポジトリに触れずに試す:**
```sh
REPO=$(bash scripts/make_fixture.sh)   # branch・merge・remote・tag・stash・dirty な working tree を用意
cargo run -- "$REPO"
```
</details>

## 🧑‍💻 開発

<details>
<summary><b>テスト・ドキュメント・v1.0 に向けた再設計</b></summary>

```sh
cargo test --workspace
```

- 設計ドキュメント: [docs/requirements.md](docs/requirements.md) · [docs/architecture.md](docs/architecture.md) · [ADR](docs/adr/)
- **実リポジトリに対してテストしないこと** — `scripts/make_fixture.sh` か tempdir を使ってください。`KAGI_*` 環境変数は headless テスト用のものです。
- Kagi は現在、安全第一の設計を規約ではなく型システムで担保できるよう、レイヤ分けした Cargo workspace へと再設計を進めています。詳しくは [docs/rearch/](docs/rearch/) と [ADR 0072 以降](docs/adr/)を参照してください。守るべき中核の不変条件は、UI が `git2` に直接触れず、すべての Git 操作が `plan → confirm → preflight → execute → verify → log` パイプラインを通ること(これも CI の grep ゲートで担保しています)。
</details>

## 📄 ライセンス

[MIT](LICENSE)。同梱のターミナルコンポーネント(`vendor/gpui-terminal`)は上流で MIT OR Apache-2.0 のデュアルライセンスであり、本プロジェクトでは MIT として利用しています。
