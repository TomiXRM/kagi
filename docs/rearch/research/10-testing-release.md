# 10. テストアーキテクチャ & リリース配布 — 現状調査と v1.0 設計方針

> NOTE (2026-06-22): `src/git` was extracted to `crates/kagi-git` in ADR-0115; paths below describe the pre-extraction layout.

- 調査日: 2026-06-14 / research sub-agent #10 (PM-led re-architecture)
- DOMAIN: test architecture, fixture repos, headless testing, CI, release packaging/distribution
- branch: re-architecture
- 関連: ADR-0038(app bundling)、ADR-0047(cross-platform release)、docs/research/qa-audit-matrix.md、docs/research/zed-gpui-reuse-research.md、docs/research/openlogi-learnings.md

---

## 1. Kagi 現状

### 1.1 ワークスペース / レイヤ構成(テスト可能性の前提)

```
Cargo.toml              [workspace] members = ["xtask"] + [package] kagi (MIT)
src/lib.rs              pub mod git;  pub mod graph;     ← ライブラリ公開はこの2つだけ
src/git/                ドメイン+git-backend(plan/execute/snapshot/oplog/drafts/...)
src/graph/              コミットグラフ layout
src/ui/                 23 ファイル(mod.rs / commit_list / conflict_editor / sidebar / tabs / commands / ...)
src/main.rs             1457 行。バイナリ entry。AppState 構築 + 47 本の KAGI_* headless path
xtask/                  bundle/dmg/appimage/icon helper(748 LOC、workspace member)
tests/                  29 統合テストスイート
scripts/                make_fixture.sh / install_linux_desktop.sh / make_icon.sh / round_icon.swift
.github/workflows/      release.yml のみ(★ cargo test を回す CI は存在しない)
```

**決定的な事実**: `src/lib.rs` が公開するのは `git` と `graph` のみ。`tests/` の全 29 スイートは `use kagi::git::*` / `use kagi::graph::*` しか import していない(grep で確認済)。つまり **`src/ui/` と `src/main.rs`(= app/view 層、AppState、OperationController 相当、view-model)はユニット/統合テストから一切到達不能**。この層は KAGI_* headless harness をスクリプトから叩く E2E でしか検証されていない。`tests/` 内に `AppState` への参照はゼロ。

### 1.2 fixture パターン

二系統ある:

1. **インライン tempdir 構築(主流)**: 各スイートが自前の `git(dir, &[...])` ヘルパで `Command::new("git")` を呼び、`TempDir` 内に必要最小の repo を組む(`build_two_branch_repo` / `build_clean_repo` / `build_cherry_pick_repo` / `build_readme_conflict_repo` など)。`GIT_AUTHOR_*` / `GIT_COMMITTER_*` / `GIT_CONFIG_NOSYSTEM=1` / `HOME=dir` / `commit.gpgsign=false` を毎回セットして決定性とホスト隔離を担保。
   - 同型のヘルパ(`git` / `write_file` / `read_file` / `head_commit_id` / `build_*`)が **スイート間でコピペ重複**している(ops_test.rs 冒頭コメントが "copied from snapshot_test pattern" と明言)。これが最大の重複負債。
   - 安全方針: 「全書き込みは TempDir 内に閉じる。kagi 自身の repo や既存 repo は決して触らない」を各ファイル冒頭で約束(ops_test.rs L4-7、qa_audit_test.rs L21-22)。`assert_fsck_clean()`(qa_audit_test.rs)で ODB 整合性も検証。

2. **`scripts/make_fixture.sh`**: 「リッチな」fixture(bare remote + merge commit + tag v0.1.0 + ahead/behind + stash + dirty/untracked WT)を `/tmp` 配下にのみ生成(DEST を tempdir 配下に強制)。これは **テストハーネスからは呼ばれず、KAGI_* 手動 E2E 検証 / 開発時の目視確認の入力**として使われる(ファイル先頭コメント参照)。

### 1.3 スイート coverage map(29 suites / ~306 test fns)

| 領域 | スイート(test fn 数) |
|---|---|
| 操作 plan/execute pipeline | ops_test(41: checkout/create-branch/stash/cherry-pick)、staging(21)、conflicts(22)、amend(13)、discard(7)、pull(10)、push(7)、revert(5)、undo(8)、stash_pop(8)、delete_branch(8)、branch_menu_ops(6)、branch_sync(5)、worktree(5)、snapshot(8) |
| 表示/読み取り | diff(10)、diffstat(8)、status(6)、log(7)、compare(2)、graph_layout(12) |
| commit メッセージ系 | message_template(20)、trailers(14)、message_gen(5)、checklist(16)、drafts(7) |
| メタ/横断 | qa_audit(11)、oplog(10)、i18n(4) |

特徴: **ほぼすべてが git-backend の純ロジック(plan = in-memory dry-run / execute / preflight)に対する統合テスト**で、libgit2 + 実 git binary を tempdir に対して回す。network 系(pull/push)はロジックのみ検証し、qa_audit_test.rs は「pull/push の実ネットワークは libgit2 connect timeout で数十秒ブロックし得るので CI から意図的に除外」と明記。設計の質は高い(plan が repo state を変えないこと、preflight が plan-execute 間の HEAD/stash 変化を検出すること、dirty 時にデータを壊さないこと、を厳密に assert)。

### 1.4 KAGI_* headless harness とその脆さ

`src/main.rs` に **36 種の `KAGI_*` 環境変数**(env-var path。プロンプトの "~47" は path 数=`std::env::var` 呼び出し箇所のオーダ)が直書きされ、起動引数+env で UI フローを driveする:

- 主な制御変数: `KAGI_OPEN_REPO` / `KAGI_SELECT_FIRST` / `KAGI_JUMP` / `KAGI_CONTEXT_MENU` / `KAGI_COMPARE_HEAD|WT` / `KAGI_PULL` / `KAGI_PUSH` / `KAGI_UNDO` / `KAGI_POP` / `KAGI_DISCARD(_ALL)` / `KAGI_AMEND(_MSG)` / `KAGI_DELETE_BRANCH` / `KAGI_PLAN_CHECKOUT` / `KAGI_CHECKOUT_COMMIT` / `KAGI_CREATE_BRANCH` / `KAGI_CHERRY_PICK` / `KAGI_REVERT` / `KAGI_STAGE_FILE` / `KAGI_UNSTAGE_FILE` / `KAGI_STASH_PUSH|APPLY` / `KAGI_PLAN_WORKTREE` / `KAGI_MENU_DUMP` / `KAGI_NO_RESTORE` / 表示系 `KAGI_THEME` / `KAGI_LANG` / `KAGI_COMPACT` / `KAGI_BOTTOM_PANEL` / `KAGI_COMMIT_PANEL` / `KAGI_TERMINAL`。
- 実行モデル: `KAGI_AUTO_CONFIRM=1`(TEST-ONLY)が付くと plan に blocker が無い場合のみ実 execute まで進める。結果は `record_headless_op()` が `[kagi] planned:/executed:/verified:` を **stderr に print** し、外部スクリプトがその文字列を grep して合否判定する想定。

**脆さ(v1.0 で退役させる根拠)**:
1. **観測が文字列 print のみ** — 構造化された戻り値や assert ではなく、stderr 行の grep。フォーマット変更で E2E が静かに壊れる。
2. **harness ロジックが production binary (`main.rs` 1457 行) に常駐** — 本番コードに大量の test-only 分岐が混入し、main.rs を肥大化・難読化させている(`#[cfg(test)]` でもない)。
3. **env-var の組み合わせが暗黙** — 例: `KAGI_OPEN_FIRST_FILE` は `KAGI_SELECT_FIRST=1` 前提、など依存関係がコメントにしか無い。36 変数の直交性が保証されない。
4. **実ウィンドウ起動を伴う**(gpui の App を起動して途中で headless 分岐)ため CI で安定に回しづらく、`cargo test --workspace` には乗っていない。GUI 操作(クリック/メニュー/モーダル確定)を「env で擬似発火 → print を読む」で代用しており、本物の view レンダリング/イベントディスパッチは検証していない。
5. **plan-level の検証は tests/ と二重**: KAGI_PULL/PUSH/CHECKOUT 等の plan 確認は ops_test/pull_test 等が既にカバー。harness の固有価値は「AppState を介した view 結線」だけだが、それを print でしか観測できていない。

### 1.5 xtask パッケージング(748 LOC)

`xtask/src/{main,macos,linux,appimage,icon,util}.rs`。サブコマンド: `icon` / `bundle-macos`(Contents/MacOS + Info.plist + Resources/icns を**手組み**、cargo-bundle 不使用)/ `dmg-macos`(`hdiutil`)/ `bundle-linux`(tar.gz: bin + .desktop + icon)/ `bundle-appimage`(AppDir + appimagetool)。ADR-0047 の確定方針:外部 `cargo install` / network 依存を排し、worktree agent・オフライン sandbox でも再現可能。

### 1.6 CI / リリースフロー

- `.github/workflows/release.yml` のみ。`v*` タグ + 手動 dispatch トリガ。
- matrix: macos-arm64 / linux-x86_64 / linux-arm64(`ubuntu-24.04-arm`、`continue-on-error`+`fail-fast:false`)。
- 各 leg で xtask を薄く呼び artifact 生成 → SHA256SUMS → `upload-artifact` → 単一 `release` job が download-artifact(merge-multiple)→ `softprops/action-gh-release@v2` で **draft** release。署名は ad-hoc(`codesign -s -`)/未 notarize(Phase 1)。
- **★ テスト CI が無い**: PR/push で `cargo test` も `cargo clippy` も `cargo fmt --check` も回っていない。306 テストはローカル実行頼み。

---

## 2. 参考プロジェクトの実装方針

### 2.1 Zed の GPUI テスト基盤(gpui = 0.2.2、kagi と一致)

(出典: docs.rs/gpui/0.2.2、zed `crates/gpui/src/test.rs`、gpui README、gpui-component test skill)

- **`#[gpui::test]` マクロ**: コンテキストを要求するテストを専用 test dispatcher 上で回す。ForegroundExecutor / BackgroundExecutor の **テスト実装**が、任意の並列性のもとでも**決定的**にスケジューリングする(seed 注入で property-test 化も可)。`StdRng` 引数を取って randomized/iteration テストも書ける。
- **`TestAppContext`(非可視)**: 実 OS ウィンドウ無しで `App` を構築し、Entity を create/update/read できる。協調 UI 検証用に**複数コンテキスト**を要求可能。`run_until_parked()` で「全 async task が park するまで」イベントループを進め、background spawn の完了を同期点として待てる(= async 操作の決定的検証)。`observe()` で Entity 変化をストリーム化して購読。
- **`VisualTestContext`**: window/レンダリングを伴う view テスト用に `TestAppContext` を拡張。コンポーネントの描画・入力シミュレーション(クリック/キー/アクションディスパッチ)・draw/update サイクルの検証ができる。実ディスプレイ不要。
- **重要な分業指針**(gpui-component が明言): 「window や rendering が不要なら `#[gpui::test]`/`TestAppContext` を**使わず普通の `#[test]`** を書け」。= ロジックは plain unit test、結線が要る所だけ gpui::test。

### 2.2 Rust workspace テストレイアウトのベストプラクティス

- **multi-crate workspace** にして層ごとに crate を切る → 各 crate が独立に `cargo test -p` でき、依存が逆流しない。純ドメイン crate は GUI/IO 依存ゼロでミリ秒テスト。
- **共有テストユーティリティは専用 crate**(`test-support` 等、`[dev-dependencies]` か通常依存+`#[cfg(feature)]`)に切り出し、fixture builder のコピペを撲滅。Zed も `gpui` 内に test support、各 crate に `test` モジュールを持つ構成。
- `tests/`(integration)は「公開 API の利用者視点」、ロジック詳細は各 crate 内 `#[cfg(test)] mod tests`(private に到達可)で。
- CI は `cargo test --workspace --all-features` + `clippy -D warnings` + `fmt --check` を PR gate に。

---

## 3. 採用すべき設計(layered test strategy)

v1.0 の domain / git-backend / app / ui 4 層分割と1対1で**4層テスト戦略**を敷く。各層は下位のみに依存し、上に行くほどテスト数は少なく重くなる(テストピラミッド)。

### 3.1 レイヤ別テスト方針

| 層 | crate(想定) | テスト種別 | ハーネス | 速度/個数 |
|---|---|---|---|---|
| **domain**(純データ/規則: plan の不変条件、branch 名 validation、message template、trailers、checklist、graph layout) | `kagi-domain` | `#[test]` plain unit(window 不要) | 標準 cargo test | 多・極速。I/O ゼロ |
| **git-backend**(libgit2 + git binary に対する execute/snapshot/oplog/drafts) | `kagi-git` | fixture-integration(現 tests/ の主力をここへ) | 標準 cargo test + **fixture crate** | 中・tempdir I/O |
| **app**(view-model / OperationController / AppState の状態遷移・plan→confirm→execute パイプライン) | `kagi-app` | 大半は **plain unit**(view-model を gpui Entity から切り離せた分)+ 必要箇所のみ `#[gpui::test]` + `TestAppContext`(window 不要、`run_until_parked` で background execute を同期検証) | gpui::test(非可視) | 中・少 |
| **ui**(gpui View の描画/入力/アクションディスパッチ/モーダル確定) | `kagi-ui` | `#[gpui::test]` + `VisualTestContext`(描画・クリック・キー・action) | gpui::test(可視) | 少・重 |

**設計の肝**: app 層を view(gpui Entity)から分離し、OperationController/view-model を**プレーン構造体**にする。すると現在 KAGI_* harness でしか触れなかった「AppState を介した結線ロジック」の大半が `TestAppContext` すら不要な plain unit になる(gpui-component の指針「不要なら使うな」を適用)。KAGI_AUTO_CONFIRM が driveしていた「plan→preflight→background execute→verify」は app 層の controller テスト + `run_until_parked` で構造化 assert に置換できる。

### 3.2 KAGI_* harness の退役 / 薄い CLI への縮退

- **退役**: 36 の env-var path の本質は (a) plan 確認、(b) execute 確認、(c) AppState 結線確認。(a)(b) は既に tests/ と重複 → 削除。(c) は app/ui 層の gpui::test へ移管。
- **残す価値があるなら「薄い test-only CLI サブコマンド」へ**: `main.rs` から test 分岐を全撤去し、もし real-binary smoke E2E が必要なら別 bin(`kagi-headless` or `xtask e2e`)に集約。production `main.rs` には `#[cfg(test)]` でない test-only コードを一切残さない。出力は print grep ではなく **JSON(serde)で stdout** に吐き、スクリプトは構造化パースする(フォーマット変更耐性)。
- 移行は段階的に: まず app/ui の gpui::test を増やしながら、対応する KAGI_* path を1つずつ削る(各削除で main.rs が縮む)。

### 3.3 fixture-repo helper を test crate 化

- 現状コピペされている `git()/write_file()/build_two_branch_repo()/build_cherry_pick_repo()/assert_fsck_clean()` を **`kagi-test-fixtures` crate**(workspace member、`[dev-dependencies]` 経由で各層から利用)に集約。
- builder API 例: `FixtureRepo::new().branch("feature/one").commit(...).dirty(...).with_remote().with_stash().build() -> (TempDir, Repository)`。`make_fixture.sh` のリッチ fixture(remote+merge+tag+ahead/behind+stash+dirty)もこの crate の Rust builder として再実装し、シェル/Rust の二重メンテを解消(シェル版は手動目視用に残置可)。
- 隔離規約(`GIT_CONFIG_NOSYSTEM` / `HOME=tempdir` / `gpgsign=false` / TempDir 限定書き込み)を crate に1箇所で固定 = 全テストが規約を継承。

### 3.4 CI マトリクス(新規 `ci.yml`)

- **PR/push gate**(新規必須): `cargo fmt --check` / `cargo clippy --workspace --all-targets -D warnings` / **`cargo test --workspace`**。
- OS matrix: macos / ubuntu(GUI gpui::test を回すため Linux に release.yml と同じ system deps = libxkbcommon/wayland/x11/fontconfig/vulkan 等をインストール。ヘッドレス gpui::test 用に必要なら `xvfb` も検討)。
- **network 系は除外維持**: pull/push の実ネットワークテストは `#[ignore]` or feature gate で CI 既定から外す(qa_audit の既存方針を踏襲)。fixture の bare-remote(local path)に対する push/pull は OK(libgit2 connect timeout を回避できる)。
- 決定性: gpui::test の executor は seed 固定で再現可能。flaky な real-window E2E は gate に入れない。

### 3.5 packaging は不変、ただし workspace 再編に追従して移動

- xtask のロジック(手組み .app / hdiutil dmg / tar.gz / AppImage / icon)と release.yml は **方針変更なし**(ADR-0038/0047 で確定済、健全)。
- ただし `--bin target/release/kagi` / `bundle-macos` が参照する **バイナリ crate のパスが workspace 再編で変わる**(例: `kagi` package → `crates/kagi-app` の bin)。xtask の `cargo build --release -p <name>` の `-p` 名と target path、`[package.metadata.bundle]` の所在を新レイアウトに合わせて更新するだけ。release.yml に **test job への依存(`needs:`)を足す** か、ci.yml の green を release の前提にするのが望ましい(未テストのものを配布しない)。

---

## 4. 採用しない設計

- **KAGI_* env-var harness の拡張継続**: print-grep 観測 + production binary 常駐 + 暗黙 env 依存。退役対象であり、ここに投資しない。
- **全 UI を `VisualTestContext` で E2E 化**: 重く flaky。app 層を view-model に分離して plain unit / 非可視 TestAppContext に倒すのが本筋。VisualTestContext は描画・入力が本質の所だけ。
- **Zed/GPL crate(workspace/editor/git_ui)のテストコード流用**: GPL 汚染ゲート(zed-gpui-reuse-research.md)。`gpui`(Apache-2.0)の test API のみ利用、パターンは概念参照に留める。
- **cargo-bundle / create-dmg への回帰**: ADR-0047 が手組み xtask + hdiutil を確定済(オフライン/worktree 再現性のため)。戻さない。
- **fixture をシェルスクリプトに一本化**: CI から呼べず構造化観測もできない。Rust builder crate を主に、シェルは手動目視補助。
- **実ネットワーク pull/push を CI gate に**: connect timeout で HUNG クラス(qa-audit-matrix.md)。local bare-remote fixture で代替。

---

## 5. リスク

1. **app/ui 分離の難度**: AppState から view-model/OperationController を gpui Entity 非依存に切り出せるかが戦略の前提。分離が不完全だと「plain unit にできる範囲」が縮み、gpui::test/VisualTestContext 比率が上がってテストが重く/flaky に。→ re-arch 設計(#02/#03 系)との整合が必須。
2. **gpui::test の Linux CI 安定性**: VisualTestContext がヘッドレス Linux(vulkan/xkbcommon/wayland)でどこまで安定描画できるか未検証。xvfb / software 描画の要否を要 PoC。最悪 ui 層 gpui::test は macOS only に制限。
3. **gpui 0.2.2 の test API 表面の確証不足**: 一次資料(docs.rs/gpui/0.2.2)で `VisualTestContext` / `run_until_parked` / 入力シミュレーション API の**正確なシグネチャを未確認**(Web 調査は概念レベル)。実コードで早期に薄い PoC を1本書いて API を確定すべき。
4. **移行中の二重メンテ**: KAGI_* を一度に消せないため、新 gpui::test と旧 harness が一時並存 → main.rs が両方を抱える期間が出る。段階削除の規律が要る。
5. **306 テストの crate 再配置コスト**: tests/ → 各層 crate への振り分け + import パス書き換え(現 `kagi::git::*` → `kagi_git::*` 等)が機械的だが量が多い。fixture crate 化と同時に一括でやらないと二度手間。
6. **CI 実行時間**: workspace test + clippy + Linux GUI deps インストールで PR CI が長くなる。rust-cache(release.yml で既使用)を ci.yml にも適用、gpui::test は別 job に分離して並列化。

---

## 6. 未解決事項

1. **gpui 0.2.2 における `VisualTestContext` / 入力シミュレーション / `run_until_parked` の実 API**(crates.io 公開版に test support がどこまで含まれるか)。→ PoC 1本で確定する。
2. **app 層の view-model 切り出し境界**: OperationController / AppState のどこまでを gpui 非依存にするか。re-arch app 層設計サブエージェントとのインターフェース合意が必要。
3. **KAGI_* の (c) 結線確認のうち、どれが真に gpui::test 必須で、どれが plain controller unit で足りるか**の仕分け(36 path の棚卸し)。
4. **薄い E2E CLI を残すか完全退役か**: real-binary smoke を 1 本でも保持する価値があるか(リリース前 sanity)。残すなら JSON 出力 + `xtask e2e` への集約形を決定。
5. **fixture builder crate の API 設計**(`make_fixture.sh` の表現力をどこまで Rust DSL 化するか)。
6. **release.yml と ci.yml の結合**: テスト green を配布の前提条件にする `needs:` / 環境保護ルールの具体化。
7. **Linux GUI テストの CI 実行環境**(xvfb / software renderer / 専用 runner)の決定、および macOS-only fallback の線引き。
