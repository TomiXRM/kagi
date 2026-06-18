# ADR-0099: Smart Commit CLI providers (Claude Code / Codex)

- Status: Accepted
- Date: 2026-06-19
- Context: ユーザー依頼「Smart Commit のメッセージ生成に、ローカル Ollama だけ
  でなく Claude Code / Codex の CLI も選べるようにしたい。CLI を選んだときは
  コスト・クォータ・プライバシーについて分かりやすい警告を必ず出すこと」
- 関連: ADR-0044(Smart Commit / Ollama 連携)、ADR-0090(プロンプト改善)、
  `message_gen`(`MessageBackend` enum dispatch)

## Context

Smart Commit のメッセージ生成は今まで **ローカル Ollama のみ**だった
(`MessageBackend::Ollama` / `RuleBased`)。Ollama はプライバシーに優れる
(loopback、diff がマシンの外に出ない)反面、ローカルで動かせるモデルは限られ、
セットアップも必要。

一方、多くのユーザーは既に **Claude Code (`claude`) や Codex (`codex`) の CLI**
を入れており、ログイン済みで、はるかに強力なモデルにアクセスできる。これらを
Smart Commit のバックエンドとして使えれば、

- API キー管理が一切不要(CLI の既存 OAuth / サブスク認証をそのまま使う)
- Ollama より高品質な subject / body が得られる

という利点がある。トレードオフは **staged diff がローカルサンドボックスを出て
外部サービスに送られる**こと、そして **ユーザーのクォータ/課金を消費する**こと。
したがって明示的なオプトイン + 目立つ警告が必須。

## Decision

### `MessageBackend::Cli { provider }` を追加

`crates/kagi-domain/src/message_gen.rs` に純粋型を追加:

- `enum CliProvider { ClaudeCode, Codex }`。`slug()`/`from_slug()`
  (`"claude-code"`/`"codex"`)、`display_name()`、`binary()`(`claude`/`codex`)、
  `ALL` を持つ。
- `MessageBackend::Cli { provider }` バリアント。

ディスパッチ(`src/git/message_gen.rs::generate_message`)の `Cli` アームは
**Ollama アームをそのまま写経**する:`offline()` → `Err(Offline)`、stage 無し →
`Err(NoStagedChanges)`、`build_prompt(..)` で同じプロンプトを構築、`want_body`
に応じて `clean_llm_message[_multiline]`、空なら `Err(EmptyResponse)`。唯一の
違いは **トランスポート**で、HTTP POST の代わりにローカル CLI を起動する。

### 非エージェント・読み取り専用・プロンプトは stdin

CLI は **非対話・read-only・プロンプトを stdin** で渡して呼ぶ。正確な起動:

- **Claude Code**: `claude -p --output-format text`
  - print モード(`-p`)は非対話なので、ツール承認は一切発生し得ない。
  - プロンプトは **stdin**、答えは **stdout** から読む。
  - `--bare` は **付けない**。`--bare` はユーザーの OAuth / サブスク認証を
    バイパスしてしまうため、付けると認証が通らない。
  - stderr は捨てる。
- **Codex**: `codex exec -s read-only --color never -o <TMPFILE> -`
  - 末尾の `-` で codex は **stdin** から指示を読む。
  - `-s read-only` でリポジトリへの書き込みが構造的に不可能になる。
  - 最終メッセージは `-o <TMPFILE>`(`tempfile` クレート、既存依存)に書かれる
    ので、それを読み戻して結果にする。stdout/stderr は捨てる。

### デッドロック回避 + タイムアウト

プロンプト(最大 ~8KB の diff を含む)は **別スレッド**から子の stdin に書いて
書き終えたら stdin を drop(= EOF)する。子が stdout を埋めている間にメインが
stdin をブロック書き込みすると **パイプ・デッドロック**するため。

`std` の子プロセス API には時間付き wait が無いので、`try_wait()` を 100ms 間隔で
ポーリングし、~60s のデッドラインを超えたら `child.kill()` して
`Err(Http("timeout"))` を返す(kill-on-timeout)。

### 検出は PATH スキャンのみ

`cli_available(provider)` は `$PATH` の各ディレクトリを走査して実行可能な
`binary()` ファイルがあるかを見るだけ。`--version` の起動はしない(遅い・
副作用がある)。これにより UI の検出パスでインラインに呼べる(瞬時)。`offline()`
は見ない — 「インストールされているか」と「ネット遮断中か」は別問題で、オフライン
ゲートは生成時に効く。

### 設定 + 永続化

`smart_commit_provider` キー(`"ollama"` | `"claude-code"` | `"codex"`、既定
`"ollama"`)を `SmartCommitState` から永続化。CLI プロバイダ選択時はモデル
ピッカーをスキップ(モデルはプロバイダ自身が持つ)。`llm_offered()` は選択中の
プロバイダに応じて必要なバックエンド(Ollama 検出 / CLI 検出)を要求する。

## Safety / Privacy

- **read-only サンドボックス**:`-p`(Claude)/ `-s read-only`(Codex)で、CLI は
  kagi 経由でリポジトリを書き換えられない。ツール承認も print/exec モードでは発生
  しない。
- **diff は外に出る**:Ollama と違い、staged diff は外部サービスに送られる。
  よって **明示的オプトイン**(プロバイダを選ぶ操作そのもの)+ **Settings の
  目立つ警告**で同意を取る。警告は警告色(`theme().color_warning`)の枠付き
  ブロックで、(1) staged diff が外部 `claude`/`codex` CLI に送られ
  ローカル Ollama サンドボックスを出ること、(2) ユーザーのアカウント/クォータを
  使い課金され得ること、(3) kagi は CLI を非対話・read-only で動かしリポジトリを
  変更しないこと、(4) CLI のインストールとログインが必要、の4点を全て明記する。
- `KAGI_OFFLINE=1` のときは Ollama 同様、生成は一切行わず `Err(Offline)`。

## Alternatives considered

- **各プロバイダの HTTP API / SDK を直接叩く**:API キー管理が必要になり、
  ユーザーの既存 CLI 認証(OAuth/サブスク)を再利用できない。MVP では却下。
  将来 ADR で別途検討。
- **CLI をライブラリとしてリンク**:そんな安定 API は無い。サブプロセス起動が
  唯一現実的で、read-only / 非対話フラグで安全に閉じ込められる。

## Consequences

- 強力なモデルが鍵管理ゼロで使える(既存 CLI 認証を再利用)。
- コスト/クォータを消費し、Ollama よりレイテンシが高い(初回は認証更新の
  コールドスタートもあり得るため 60s の余裕を持たせた)。
- 外部バイナリへの依存。未インストール時は Settings で「not found on PATH」と
  表示し選択不可。
- 既定はあくまで Ollama / オプトイン off。CLI 失敗時は他バックエンド同様、
  静かに rule-based ドラフトへフォールバックする。
- `src/ui/` の git2 直アクセス禁止ゲートは維持(サブプロセス起動は
  `src/git/message_gen.rs` 側)。
