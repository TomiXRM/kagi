# ADR-0205: リポジトリ健全性の提案 — 検出は読み取り、有効化は plan → confirm の Operation

- Status: **Accepted**
- Date: 2026-10-01
- Related: [#358](https://github.com/TomiXRM/kagi/issues/358)（「リポジトリ健全性の提案 UI」slice のみ。git ≥ 2.52 が要る項目は対象外）、
  ADR-0119（Analyze / ecosystem）、ADR-0121 C2（Analyze crate は Git-free）、ADR-0129（typed plan text）、
  ADR-0146（`run_git` hardening）、ADR-0149 / ADR-0178（`Backend::run` が唯一の記録境界）、ADR-0196（lease / reconcile）

## 文脈

巨大リポジトリでの体感速度は、自前の高速化より git が既に持つ最適化を有効にするほうが効く。
代表は 2 つある。

- **commit-graph**: 履歴走査（グラフ・Analyze・blame）が commit object を毎回解析しなくて済む。
- **built-in fsmonitor**（`core.fsmonitor=true`）: `status` が作業ツリー全体を走査しなくて済む。

どちらも既定では無効であることが多い。ただし Kagi の存在理由は「勝手に書き換えない」ことなので、
検出して提案し、有効化は必ず確認を通す必要がある。

## 決定

1. **Analyze の第 4 軸「Health」**に置く（`EcosystemMode::Health`）。検出は履歴 mine とは別の
   軽い読み取り（`Backend::repo_health`: ファイルの mtime と config だけで、git subprocess は起動しない）。
   Analyze を開いたとき、HEAD が変わったとき、fix が成功したときに読み直す。古い読み取りが
   新しい読み取りを上書きしないよう、request token で保護する。
2. **判定は kagi-domain の pure 関数** `repo_health::assess(&HealthFacts)`。
   - commit-graph（単一ファイル、または split chain）が無い → `CommitGraphMissing`。
   - HEAD の committer time より commit-graph ファイルの mtime が古い → `CommitGraphStale`。
     mtime 比較は、HEAD 以降に書かれたグラフを「古い」と誤判定しない安全側のヒューリスティック。
     古い committer time のコミットが後から fetch された場合は検出できないが、不要な書き直しは提案しない。
   - `core.fsmonitor` がどの config level にも無い、かつ built-in daemon がある platform（macOS / Windows）
     → `FsmonitorUnset`。明示的な `false` はユーザーの選択として尊重する。
   - bare repository と unborn HEAD には何も提案しない。
3. **有効化は通常の `Operation`**（`WriteCommitGraph` / `EnableFsmonitor`）。
   `crates/kagi-git/src/ops/repo_health.rs` の `plan_ / preflight_ / execute_ / verify_` を
   `Backend::run_recorded` で実行し、oplog に記録する。Health 軸のボタンは host に
   `HealthFixRequested` を送るだけで、host が plan して共有 plan card（`ActiveModal::RepoHealth`）を開く。
   実行は confirm（ボタン / root Enter）の後だけ。
   - commit-graph: `git commit-graph write --reachable` を hardening 付きの `run_git` で実行し、
     終了コードとファイルの存在を verify する。unborn HEAD は blocker。
   - fsmonitor: git2 で local config に `core.fsmonitor=true` を 1 key だけ書き、ディスク上の
     local config を開き直して verify する。既存の値（どの level でも）と非対応 platform は blocker。
   - plan は `equivalent_command`（上記の git コマンド）と recovery を必ず持つ。recovery は
     commit-graph ファイルの削除（キャッシュなので無くても git は動く）と
     `git config --unset core.fsmonitor`。どちらも non-destructive。
4. **Kagi 自身の git は影響を受けない**。`run_git` は常に `-c core.fsmonitor=` を付けるため、
   有効化で速くなるのはユーザーが直接使う git だけ。Kagi 自身の fsmonitor 利用は本 ADR の範囲外。
5. 新しい slug（`write-commit-graph` / `enable-fsmonitor`）は local だけの write として
   `writes_only_locally` に入れる。巨大 repo で commit-graph の書き込みが 60 秒の timeout を超えて
   Unknown になっても、local の読み取りで reconcile できる。

## 却下した案

- **Analyze を開いたら自動で有効化する**: plan → confirm を迂回する（#358 §5 で却下済み）。
- **`git commit-graph verify` で staleness を判定する**: verify は整合性を検査するだけで、
  HEAD がグラフに含まれるかは答えない。読み取りのたびに subprocess も起動する。
- **`git maintenance` の提案**: 閾値の合意が無く（#358 §5 論点）、`is-needed` は git 2.53 以上が必要。

## 結果

- Analyze に Health タブが増える。所見ごとに EN/JA の説明と「有効化…」ボタンを表示する。
- ボタンを押しても plan card が開くだけ。確定するまで repository・oplog は変わらない（Tier A `repo_health_proposal`）。
- `Operation` に 2 variant、`PlanNote` / `PlanTitle` / `RecoveryKind` に `Maintenance` category が増える。
