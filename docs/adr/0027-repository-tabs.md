# ADR-0027: Repository Tabs(複数リポジトリ切り替え)

- Status: Accepted / Date: 2026-06-12

## Decision

- **tab はヘッダツールバーの上**に独立した strip として置く(GitKraken 同様)。
  各 tab: repo 名(truncate + tooltip でフルパス)/ active 強調 / × close / 右端に [+](picker)
- **状態モデルは「軽量 tab 記述子 + 単一の重量状態」**:
  ```rust
  pub struct RepoTab { pub path: PathBuf, pub name: String }
  // KagiApp に: pub tabs: Vec<RepoTab>, pub active_tab: usize
  ```
  KagiApp の既存 per-repo 状態(rows/details/diff_cache/...)は**作り直す**:
  `switch_repo(index)` = repo_path 差し替え → snapshot 再構築(既存 from_snapshot/reload の
  機構を流用)→ per-repo UI 状態のリセット(selection / diff_cache / main_diff / compare /
  modals / commit_panel)。tab ごとの selection・scroll 保持は later
- **同一 repo を二重に開かない**: 既存 tab があればそれに switch
- **tab close**: 状態破棄のみ(repo には何もしない)。最後の tab を閉じたら welcome 画面(ADR-0028)
- **watcher**: `watcher_generation: u64` を導入。switch/open で generation を進めて
  新 watcher を arm し、旧 loop は generation 不一致を検知して自然終了する
  (spawn は run_app 固定から `arm_watcher(&mut self, cx)` メソッドへ移す)
- **terminal**: session は `HashMap<PathBuf, KagiTerminalSession>` で tab 横断保持
  (PTY は生かしたまま、bottom panel は active repo の session を表示。lazy 生成は既存どおり)
- **永続化(開いていた tab の復元)は later**(設定ファイル導入時にまとめて)
- headless: `KAGI_OPEN_REPO=<path>`(tab 追加 + switch)、
  ログ `[kagi] tabs: n=<N> active=<i> <name>`

## Consequences

- oplog の repo フィールドは既存どおり操作時の repo_path を記録(tab 切替自体は記録しない)
- toast / busy_op はアプリ全域の状態として tab 切替後も維持(busy 中の switch は許可するが、
  busy 操作の finish はその repo_path に対して記録されるため安全)
- 既存 headless 経路(引数1つで起動)は「初期 tab が1枚ある状態」として互換維持
