# AI native 開発の流行りと git の使い方 — 外部サーベイ
調査日: 2026-09-03 / 担当スライス: AI native 開発の流行りと git の使い方

> 注: Kagi 本体のファイルは一切変更していない。参照は読み取りのみ。
> Kagi 側の根拠として読んだファイル: `AGENTS.md`, `src/headless.rs`, `src/main.rs`,
> `src/single_instance.rs`, `crates/kagi-git/src/oplog.rs`,
> `crates/kagi-git/src/message_gen.rs`, `docs/adr/0096-klog-contract-channel.md`,
> `docs/adr/0104-enforced-operation-pipeline.md`。

---

## 1. サマリ

Kagi に効く上位5点:

1. **エージェントは既に「worktree ネイティブ」になった。** Claude Code は `claude --worktree <name>` / `EnterWorktree` ツール / subagent の `isolation: worktree` を一級機能として持ち、`.claude/worktrees/<name>` に `worktree-<name>` ブランチを切る。Kagi の worktree 管理はもう「上級者機能」ではなく**主戦場**。エージェント由来 worktree を第一級で可視化・掃除する UI が刺さる。
2. **git trailer がエージェント来歴のメタデータ層になった。** Amp は `Co-authored-by: Amp <amp@ampcode.com>` に加え **`Amp-Thread-ID:` trailer にスレッド URL を埋める**。Claude Code は `attribution.commit` で trailer 全文を差し替え可能。Kagi のコミットグラフが trailer をパースして「どのエージェント／どの会話が生んだコミットか」を表示できれば、他の GUI に無い価値になる。
3. **Kagi 自身を MCP server にするのが最大のレバレッジ。** 既存の MCP git server (`mcp-server-git`) は `git_reset` を持つ一方 `git_worktree_*`・conflict 解決・oplog・undo を一切持たない。Kagi は「破壊的操作が存在しない git backend + plan/preflight/verify/oplog」を既に持っているので、**"安全な git MCP server" というニッチが空席**。
4. **MCP の `destructiveHint` / `readOnlyHint` は既に製品レベルで効いている。** Codex は「destructive annotation を持つ MCP tool call は必ず承認を要求（read annotation が優先）」と明記。Kagi の「二段確認」ポリシーは MCP annotation にほぼ 1:1 で写像できる。annotation を正しく付けるだけで、エージェント側のガードレールに乗れる。
5. **`Backend::run` は oplog を書かない**（oplog 記録は UI の `record_op` に residing、ADR-0104 の "Consequences"）。これは MCP/ヘッドレス面を作る前に必ず埋めるべき穴。エージェントが Kagi 経由で書き込んだ操作が oplog に載らなければ「undo できる」という製品の約束が崩れる。

---

## 2. 詳細

### 2.1 エージェント × git のワークフロー

#### Claude Code — worktree が一級機能 / checkpoint は git と別レイヤ
- **何か**: Anthropic の CLI コーディングエージェント。並列実行・分離・巻き戻しを OS/git 両レイヤで持つ。
- **出典**:
  - https://code.claude.com/docs/en/worktrees (`--worktree`, `EnterWorktree`, `isolation: worktree`, `.worktreeinclude`, `worktree.baseRef`)
  - https://code.claude.com/docs/en/checkpointing (`/rewind`, Esc×2)
  - https://code.claude.com/docs/en/common-workflows (並列セッション、plan mode、`claude -p`)
  - https://code.claude.com/docs/en/agent-view (`claude agents`, 背景セッション一覧)
- **仕組み**:
  - `claude --worktree feature-auth` → `.claude/worktrees/feature-auth/` に **`worktree-feature-auth`** ブランチで worktree を作る。名前省略時は `bright-running-fox` のような自動生成名。
  - `--worktree "#1234"` / PR URL を渡すと `origin` から `pull/<n>/head`（GitLab は `merge-requests/<n>/head`）を fetch して `.claude/worktrees/pr-<number>` を作る。
  - `worktree.baseRef` 設定: `"fresh"`（既定、リモート default branch から。24h 以上 fetch していなければ 5 秒上限で fetch）/ `"head"`（ローカル HEAD から）。
  - `.worktreeinclude`（.gitignore 構文）で **gitignore 済みファイルのみ** を新 worktree にコピー（`.env` 等）。tracked ファイルは複製しない。
  - **worktree 分離は git だけでなくツール呼び出しレベルで強制**: main checkout へのファイル編集、cwd が main checkout に解決される bash、`git -C` / `--git-dir` / `GIT_DIR` / `GIT_WORK_TREE` によるリダイレクト、追跡不能なシェル構文（brace expansion, unquoted heredoc）を**ブロック**する。
  - 作業中は worktree に **`git worktree lock`** を掛け、終了時に解放。killed session の lock は定期 sweep が解放（v2.1.210+）。Claude が作った worktree には git metadata にマーカーを書き、マーカーの無い worktree は sweep が触らない（v2.1.246+）。
  - subagent 単位の分離: `.claude/agents/*.md` の frontmatter に `isolation: worktree`。
  - **checkpoint は git ではない**: user prompt 毎にファイルスナップショットを取る（直近 100 件）。bash 由来の変更・subagent の編集・symlink/hardlink は復元対象外。docs 自身が "Not a replacement for version control" と明記。
  - PR 連携: `gh pr create` / `glab mr create` で作ったセッションは PR に紐づき、`claude --from-pr 1234` で復帰できる。agent view の行に `#1234` ラベルが出て、色で PR 状態（黄=checks/review 待ち、緑=通過、紫=merged、灰=draft/closed）を示す。
- **Kagi への示唆**: (a) `.claude/worktrees/` / `worktree-*` ブランチを **「エージェント worktree」として分類表示**。(b) `git worktree lock` 状態とロック理由を worktree パネルに出す（Kagi は既に lock/unlock を持つので表示追加のみ）。(c) checkpoint が git 外である事実は Kagi の商機 — 「エージェント試行を git に落とす」機能（下記 3.-#7）。
- **難易度**: 表示系 S / 分類ロジック M

#### OpenAI Codex — `.git` を read-only 保護、annotation ベース承認
- **何か**: OpenAI の CLI / IDE / cloud エージェント。
- **出典**: https://learn.chatgpt.com/docs/agent-approvals-security （+ `.md` 版）
- **仕組み**:
  - sandbox mode × approval policy の 2 層。既定 `Auto` = `--sandbox workspace-write --ask-for-approval on-request`、network access は既定オフ。
  - **`workspace-write` の writable root 内でも `<root>/.git` は read-only 保護**（`gitdir:` ポインタファイルの解決先も、再帰的に）。`.agents` / `.codex` も同様。→ **エージェントは `.git` を直接書けないので、git 操作はコマンド or 外部ツール経由になる。ここが MCP server の入り込む余地。**
  - 起動時に「version-controlled folder なら `Auto`、そうでなければ `read-only`」を推奨する。git 管理下かどうかが安全性の判断材料になっている。
  - **「Destructive app/MCP tool calls always require approval when the tool advertises a destructive annotation (unless the tool advertises a read annotation, which takes priority)」** — MCP annotation がそのまま承認 UI を駆動している。
  - `approvals_reviewer = "auto_review"` で承認要求を reviewer agent に回せる。reviewer policy は data exfiltration / credential probing / persistent security weakening / **destructive actions** をチェック。critical-risk は deny、prompt-build や parse 失敗は fail closed。
- **Kagi への示唆**: Kagi が MCP tool を出すとき **annotation を正確に付けるだけで Codex/Claude 側の承認 UI が自動で働く**。`kagi_push_force` のような tool は存在させない（そもそも Kagi に無い）が、`kagi_discard`・`kagi_reset_soft`・`kagi_delete_branch` には `destructiveHint: true` を付ける。
- **難易度**: S（annotation を付けるだけ）

#### Cursor — checkpoint は git と分離、明示的に「git を使え」
- **何か**: AI IDE。
- **出典**: https://cursor.com/docs/agent/overview
- **仕組み**: Agent が「significant changes」の前にコードベースのスナップショット（checkpoint）を自動生成。chat timeline の checkpoint をクリックしてプレビュー → restore。**restore はファイルのみ戻し、会話は消さない**。docs 上で「Checkpoints are stored locally and separate from Git. Only use them for undoing Agent changes; use Git for permanent version control.」と明記。`/goal` で長寿命目標、queue/steer で走行中のエージェントへの割り込み。
- **Kagi への示唆**: 主要 IDE 3 種（Claude Code / Cursor / Codex）が揃って「checkpoint は git じゃない」と言っている。→ **Kagi が「エージェント試行の git への正しい落とし方」を提供する空白がある**（3.-#7, #8）。
- **難易度**: —（示唆のみ）

#### GitHub Copilot cloud agent（旧 coding agent）
- **何か**: GitHub Actions 上の ephemeral 環境で自律実行し、ブランチ + PR を作るクラウドエージェント。
- **出典**: https://docs.github.com/en/copilot/concepts/agents/cloud-agent/about-cloud-agent
- **仕組み**:
  - ブランチ作成・コミットメッセージ作成・push を**自動化**。**1 タスク = 1 ブランチ = ちょうど 1 PR**（複数ブランチ不可）。1 セッションの実行上限は **59 分（ハード制限）**。
  - 単一リポジトリのみ（cross-repo 不可）。GitHub MCP server と Playwright MCP server が既定で有効。
  - `@copilot` メンションで既存 PR に追加変更を依頼できる。
  - **「特定の commit author のみを許すルールセットは Copilot cloud agent の PR 作成/更新をブロックする」** → ruleset に bypass actor として Copilot を追加する必要がある。エージェント時代の branch protection 運用課題。
  - hooks（agent 実行中の key point でシェルコマンド）、custom agents、skills、custom instructions で拡張。
  - `resolve merge conflicts` が公式の能力リストに入っている。
- **Kagi への示唆**: Kagi の PR 一覧に **「このPRはエージェント作成」バッジ**（author が `Copilot`/`app/copilot-swe-agent` 等）。加えて「エージェント PR は本文が長く commit が細かい」という前提で、PR モードのデフォルト表示を調整する余地。
- **難易度**: S

#### Google Jules — commit authorship を選べる
- **何か**: Google の非同期クラウドコーディングエージェント（VM 上で実行 → ブランチ公開 → PR）。
- **出典**: https://jules.google/docs/ , https://jules.google/docs/changelog/ , https://github.com/google-labs-code/jules-action （`starting_branch` は既定 `main`, `include_last_commit` オプション）
- **仕組み**: 開始ブランチを選択（既定 default branch）。完了後に別ブランチ + コミットメッセージを提示 → 人間が調整 → **Publish branch** → 続けて PR を開ける。**commit authorship を 3 モードから選べる: (1) Jules が単独 author（既定）、(2) 人間 + Jules の co-author、(3) 人間が単独 author（Jules は人間の identity で commit）**。
- **Kagi への示唆**: 「エージェントの識別」が author/committer/trailer のどこに現れるかは**プロダクトごとにバラバラ**。Kagi は 3 経路（author, committer, trailer）を全部見て来歴を判定する必要がある（3.-#2）。
- **難易度**: M

#### Amp — trailer にスレッド URL、`Ship` の挙動が設定可能
- **何か**: Sourcegraph の frontier agent。リモート環境「orb」で実行。
- **出典**:
  - https://ampcode.com/docs/markdown/github （lastModified 2026-09-01）
  - https://ampcode.com/docs/markdown/orbs/shipping （lastModified 2026-08-27）
- **仕組み**:
  - orb 起動時に `/home/user/workspace/repo` へ **shallow single-branch clone**。長寿命トークンは orb に置かず、git credential helper が毎回 Amp に short-lived token を要求する。`gh` はプリインストール済み・認証済み。SSH URL は HTTPS に書き換え。
  - **commit identity ポリシー（workspace 単位）**: `Amp`（author = `Amp <amp@ampcode.com>`、スレッド作成者を `Co-authored-by:` で追加）/ `Amp Account`（各人の identity）/ `User Choice`。
  - 人間が author の場合 **`Co-authored-by: Amp <amp@ampcode.com>` trailer** を付ける。`AMP_DISABLE_AMP_COAUTHOR_TRAILER=1` で無効化。
  - **さらに全コミットに `Amp-Thread-ID:` trailer を付け、値はスレッドの URL**。「anyone reading the Git history can open the conversation that produced the change」と明記。
  - **commit signing**: Amp がサーバ側で SSH keypair を保持し、orb 内の git が Amp を呼んで署名（秘密鍵は orb に入らない）。GitHub で Verified にするには公開鍵を **Signing Key** として登録。
  - **Ship Behavior（3 択）**: `Ship`（既定、trunk-based: commit → fetch+rebase onto `origin/main` → 全テスト → base branch に直 push → 失敗したら rebase して再 push → thread をアーカイブ）/ `Push to Branch`（feature branch を作って push、GitHub なら PR URL を報告）/ `Custom Ship`（10,000 字までの自由プロンプト、`.agents/ship.md` にファイルで置いて `amp projects update --custom-ship-prompt-file` で登録）。
  - リポジトリ規約（コミットメッセージ形式・実行すべきテスト）は Ship prompt ではなく **`AGENTS.md`** に置け、と docs が指示。
- **Kagi への示唆**: **`Amp-Thread-ID:` 型の「会話へのリンク trailer」が実在する**。Kagi のコミット詳細ペインが trailer 内 URL をリンクとして描画するだけで、コミット → エージェント会話への往復が成立する（3.-#3）。「trunk-based 直 push が既定」というのも重要 — エージェント時代は `main` が高頻度で動くので、Kagi の fetch-age indicator / force-with-lease の価値が上がる。
- **難易度**: trailer 表示 S / リンク化 S

#### 並列エージェント運用のためのサンドボックス
| ツール | 分離手段 | ブランチ規約 | 成果の取り込み | 人間の介入 | 出典 |
|---|---|---|---|---|---|
| **dagger/container-use** | Dagger コンテナ **+** git ブランチ（両方） | `cu-<env-id>`（env-id は `fancy-mallard` のような2語） | `container-use merge <env>`（履歴保持）/ `container-use apply <env>`（コミットせず staged 変更として適用）、両方 `--delete` 可 | `container-use terminal <env>` でコンテナ内シェル、`container-use watch` でライブ監視、`container-use log --patch`、`container-use diff` | https://github.com/dagger/container-use , https://raw.githubusercontent.com/dagger/container-use/main/docs/cli-reference.mdx |
| **claude-squad** (`cs`) | **tmux セッション + git worktree** | README に明記なし（worktree ごとに独自ブランチ） | `s` = commit して GitHub に push / `c` = checkout（commit してセッション一時停止）→ 手元で確認 | `↵`/`o` で attach、`tab` で preview/diff 切替、`-y` で全インスタンス auto-accept | https://github.com/smtg-ai/claude-squad (AGPL-3.0, 前提: tmux + gh) |
| **vibe-kanban** | git worktree（`DISABLE_WORKTREE_CLEANUP` 環境変数で cleanup 停止可）。Rust + TS 実装 | 明記なし（workspace ごとにブランチ） | AI 生成 description で PR を開き GitHub でレビュー → merge | diff に**インラインコメント**を書いてエージェントに送る、内蔵ブラウザで preview | https://github.com/BloopAI/vibe-kanban （**2026-09 時点で "sunsetting" 告知あり**: https://www.vibekanban.com/blog/shutdown） |
| **crystal** | git worktree | — | — | — | **2026-02 に deprecated → Nimbalyst に改名**。Nimbalyst の機能リストに "Git worktree isolation for safer parallel AI coding sessions" が残る: https://github.com/stravu/crystal , https://nimbalyst.com/ |
| **Claude Code agent view** | 背景セッション（それぞれ worktree 可） | `worktree-<name>` | セッションが自分で PR を開く。行に `#N` ラベル | `Space` で peek+返信、`Enter` で attach、`Ctrl+X` で停止/削除、`Ctrl+T` で pin | https://code.claude.com/docs/en/agent-view |

- **container-use への補足**: MCP server として動く（`container-use stdio`）。`cd repo && claude mcp add container-use -- container-use stdio` で登録し、`rules/agent.md` を `CLAUDE.md` に追記する運用。README は「complete command history and logs of what agents actually did, not just what they claim」を売りにしている — **エージェントの自己申告ではなく実際の操作履歴を見せる**という価値提案。
- **Kagi への示唆**: この表の右 2 列（成果の取り込み / 人間の介入）が**そのまま Kagi の未実装領域**。Kagi は worktree 管理と repo タブと diff とコンフリクトエディタを既に持っているので、「複数 worktree の diff を横並び比較して 1 つを選ぶ」UI に一番近い位置にいる（3.-#5, #6）。
- **難易度**: 比較 UI は L

#### mergiraf — 構文認識 merge driver
- **何か**: tree-sitter ベースの構文認識 git merge driver（Rust）。
- **出典**: https://mergiraf.org/ , https://mergiraf.org/usage.html
- **仕組み**:
  - `merge.mergiraf.driver = 'mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L'` + gitattributes に `* merge=mergiraf`（または `*.py merge=mergiraf`）。**merge だけでなく rebase / cherry-pick / revert にも効く**。Git v2.44.0+ 推奨。
  - `merge.conflictStyle = diff3` を前提とする。
  - 3 つの結果: (1) 行ベースで衝突なし → そのまま返す（高速）、(2) 行ベースで衝突するが構文認識で全解決 → **`mergiraf review <merge-id>` でレビューを促す**（例: `INFO Mergiraf: Solved 2 conflicts. Review with: mergiraf review geolocation.cpp_o0i2JL8B`）、(3) 解決不能 → conflict marker を残す。
  - `mergiraf=0 git rebase ...` で一時無効化。`mergiraf solve <file>` で事後的に解決。`--compact`/`-c` で「不一致部分だけを囲む」狭い conflict 表示（要再フォーマット）。
  - 言語指定は `--language`、または gitattributes の `mergiraf.language` / `linguist-language`。`mergiraf languages --gitattributes` で対応拡張子一覧を生成。`--allow-parse-errors`（C/C++/HTML は既定 on）。
  - 設計方針として **「Don't sweep conflicts under the rug」** — 疑わしい場合は conflict marker を残す側に倒す。Jujutsu は既定設定を同梱。
- **Kagi への示唆**: Kagi の conflict editor が既に **diff3 を扱える**ので、mergiraf を「オプトインの前処理」として噛ませられる。特に「(2) 自動解決したがレビューを促す」状態は **Kagi の conflict editor が最も価値を出せる状態**（人間が hunk 単位で ours/theirs/manual を選べる UI が既にある）。`mergiraf review` を Kagi の conflict editor に置き換える提案が成立する（3.-#9）。
- **難易度**: M

---

### 2.2 規約・メタデータの流行り

#### AGENTS.md — 事実上の標準に収斂した
- **何か**: コーディングエージェント向け指示ファイルのオープンフォーマット。「README for agents」。
- **出典**: https://agents.md/ , https://github.com/agentsmd/agents.md
- **仕組み**:
  - **標準 Markdown、必須フィールドなし**。任意の見出しを使える。エージェントは単にテキストをパースする。
  - **monorepo は nested AGENTS.md**: 「編集対象ファイルに最も近い AGENTS.md が勝つ。ユーザの明示的な chat prompt が全てを上書きする」。OpenAI 本体リポジトリは 88 個の AGENTS.md を持つ（サイト記載）。
  - AGENTS.md に列挙したテストコマンドは **エージェントが自動で実行し、失敗を直そうとする**。
  - 移行方法として公式に `mv AGENT.md AGENTS.md && ln -s AGENTS.md AGENT.md`（symlink 互換）を推奨。
  - **2026-09 時点で 60k+ の OSS が採用**（GitHub code search: `path:AGENTS.md NOT is:fork NOT is:archived`）。
  - **Linux Foundation 傘下の Agentic AI Foundation (https://aaif.io) が stewardship を持つ**。
  - 対応表明: Codex, Jules, Factory, Aider, goose, opencode, Zed, Warp, VS Code, Devin, UiPath, Junie, Amp, Cursor, RooCode, Gemini CLI, Kilo Code, Phoenix, Semgrep, GitHub Copilot coding agent, Ona, Windsurf, Augment Code。
  - Aider は `.aider.conf.yml` に `read: AGENTS.md`、Gemini CLI は `.gemini/settings.json` に `{"context": {"fileName": "AGENTS.md"}}` で対応。
- **Kagi への示唆**: **Kagi 自身のリポジトリは既に `AGENTS.md` を持っている（良い）**。プロダクト機能としての示唆は別: (a) `AGENTS.md` / `CLAUDE.md` / `.cursorrules` / `.github/copilot-instructions.md` を **Kagi のサイドバーで「Agent config」として一箇所に集約表示**（Kagi は埋め込みエディタと Markdown レンダリングを既に持つ）。(b) nested AGENTS.md の「最も近いものが勝つ」解決順を可視化するツリー。(c) これらのファイルが変更された diff に**専用バッジ**（規約変更はレビュー優先度が高い）。
- **難易度**: (a) S / (b) M

#### CLAUDE.md / `.cursorrules` / `copilot-instructions.md` の現在地
- **何か**: プロダクト固有の指示ファイル。
- **出典**: https://code.claude.com/docs/en/worktrees （worktree が `CLAUDE.md` と settings を持って移動する記述）, https://docs.github.com/en/copilot/concepts/prompting/response-customization , https://agents.md/
- **仕組み**: AGENTS.md への収斂が進む一方、**プロダクト固有ファイルは消えていない**。Claude Code は `CLAUDE.md` + `.claude/` ディレクトリ（agents/, skills/, worktrees/, settings.json, settings.local.json）。Copilot は repository custom instructions + org 単位の instructions。Cursor は `.cursorrules` から `.cursor/rules/*.mdc`（Rules）へ移行済み。
- **Kagi への示唆**: 「agent config ファイル群」は**1リポジトリで 5〜10 個に膨れる**。Kagi の sidebar に専用セクションを置く価値がある。
- **難易度**: S

#### Conventional Commits / gitmoji
- **何か**: コミットメッセージ規約。
- **出典**: https://www.conventionalcommits.org/en/v1.0.0/ , https://gitmoji.dev/
- **仕組み**:
  - **仕様は v1.0.0 が最新**（2026-09 時点で v1.0.0 以降の新バージョンは無い。ページの Versions ドロップダウンは v1.0.0 と v1.0.0-beta.1〜4 のみ）。`<type>[optional scope]: <description>` + body + footer(s)。
  - **footer は明示的に git trailer 慣習に依拠**: 仕様 §8「Each footer MUST consist of a word token, followed by either a `: ` or ` #` separator（this is inspired by the git trailer convention）」、§9「token MUST use `-` in place of whitespace（例 `Acked-by`）。例外は `BREAKING CHANGE`」。
  - 破壊的変更: footer に `BREAKING CHANGE:`、または type/scope の直後に `!`（`feat(api)!:`）。`BREAKING-CHANGE` は `BREAKING CHANGE` の同義。
  - revert について仕様は明示的に定義せず、`revert:` type + `Refs: 676104e, a215868` footer を「推奨の一つ」として挙げるのみ。
  - gitmoji は opencommit の `OCO_EMOJI` / `--fgm` に組み込まれる程度に生存。opencommit は既定で 10 個のサブセット（🐛✨📝🚀✅♻️⬆️🔧🌐💡）を使い、`--fgm` で full spec。
- **Kagi への示唆**: Kagi の commit panel が Conventional Commits の **type / scope / `!` / footer を構造的に**扱えると良い（現状の `rule_based` はメッセージ文字列を返すだけ）。特に `!` と `BREAKING CHANGE:` はコミットグラフ上でバッジにする価値がある。
- **難易度**: パース+バッジ S / 入力支援 M

#### AI 生成コミットの trailer 慣習
- **何か**: 「このコミットはエージェントが作った」を機械可読に残す方法。
- **出典**:
  - `git interpret-trailers`: https://git-scm.com/docs/git-interpret-trailers （**2.54.0 で最終更新、2026-04-20**。2.55.0 は変更なし）
  - Claude Code: https://code.claude.com/docs/en/settings-reference （`attribution`, `includeCoAuthoredBy`, `includeGitInstructions`）
  - Amp: https://ampcode.com/docs/markdown/github
  - Jules: https://jules.google/docs/changelog/
- **仕組み**:
  - **`git interpret-trailers` の仕様**: RFC 822 風の `key: value` 行をコミットメッセージ末尾に置く。`--parse` は設定/CLI 引数の影響を受けず入力の trailer だけを出力。`--in-place`, `--trim-empty`, `--trailer <key>[(=|:)<value>]`。`trailer.<key-alias>.key` で短縮エイリアス（例 `trailer.sign.key = "Signed-off-by: "` → `--trailer="sign: foo"`）。
  - **既存 trailer の検出ルール（重要）**: 「(i) 全行が trailer である、または (ii) Git 生成 or ユーザ設定の trailer を少なくとも 1 つ含み、かつ 25% 以上が trailer であるような、1 行以上の行グループ。そのグループは 1 行以上の空行（または空白のみの行）に先行されていなければならない」。→ **Kagi が trailer をパースするなら、この 25% ルールと空行前置を守る必要がある**（単純な「末尾の `Key: value` 行を拾う」では git と挙動が食い違う）。
  - **Claude Code の attribution**: 既定は `Co-Authored-By: <name> <noreply@anthropic.com>`。`<name>` は**セッションの active model 名**（例 `Claude Sonnet 5`）。Claude モデルと認識できるが版が確定できないときは `Claude` 単独、モデル ID がどの Claude にも一致しない（第三者 `ANTHROPIC_BASE_URL` 経由など）ときは `Claude Code`。
    - `attribution` は `commit` / `pr` の文字列 + `sessionUrl` (Boolean) を持つオブジェクト。全部隠すには `commit`/`pr` を空文字列、`sessionUrl` を `false`。
    - `includeCoAuthoredBy` は **v2.0.62 で deprecated**（`attribution` が置き換え）。旧設定ファイルの `includeCoAuthoredBy: false` は今も尊重されるが、`attribution.commit`/`attribution.pr` を設定すると無視される。
    - `includeGitInstructions` でシステムプロンプトから組み込みの commit/PR 指示を外せる。
  - **Amp**: `Co-authored-by: Amp <amp@ampcode.com>`（`AMP_DISABLE_AMP_COAUTHOR_TRAILER=1` で無効化）+ **`Amp-Thread-ID: <thread URL>`**。
  - **Jules**: author/co-author/人間単独 の 3 モードを UI で選択。
  - `Generated-with:` / `Assisted-by:` を標準化する動きは **2026-09 時点で見つけられなかった**。実態は各社が独自 trailer を撒いている状態。
- **Kagi への示唆**: **これが Kagi 最大の差別化ポイント。** コミットグラフの各行に「どのエージェント／どの会話由来か」を出せる GUI は他に無い（GitHub の PR ページも trailer を特別扱いしない）。`Amp-Thread-ID` のような URL trailer をクリック可能にする、`Co-Authored-By` のモデル名で集計する（Analyze の ownership に「AI vs human」軸を足す）等。
- **難易度**: パース+表示 S / Analyze 統合 M

#### spec-driven development（GitHub spec-kit）
- **何か**: 「コードの前に仕様を書く」ワークフローをスラッシュコマンド群として配布するツールキット。
- **出典**: https://github.com/github/spec-kit （**1.0.0 リリース済み、初コミットから1年**。README の NOTE 参照）
- **仕組み**:
  - `uv tool install specify-cli --from git+https://github.com/github/spec-kit.git@vX.Y.Z` → `specify init my-project --integration copilot`。
  - コアフロー: `/speckit.constitution`（プロジェクト原則、1回だけ）→ `/speckit.specify`（何を作るか）→ `/speckit.plan`（技術スタック）→ `/speckit.tasks`（タスク分解）→ `/speckit.implement` → **`/speckit.converge`（実装を spec/plan/tasks に照合し、残作業を新タスクとして追記）**。converge が "Converged" を返すまで implement/converge を繰り返す。
  - オプション: `/speckit.clarify`（未確定領域の明確化）、`/speckit.analyze`（成果物間の整合性・カバレッジ分析）、`/speckit.checklist`（「英語のユニットテスト」）、`/speckit.taskstoissues`（タスクを GitHub issue 化）。
  - 拡張: `specify extension add bug`（assess → fix → test）、`specify extension add assess`（intake → research → define → shape → decide で go/needs-clarification/kill を出す）。
  - テンプレート解決は 4 段のスタック（project-local overrides > presets > extensions > core）を **runtime に top-down で walk して最初にマッチしたものを使う**。
  - 状態は `.specify/`（memory/, templates/, presets/, extensions/）に置かれ **git 管理される**。30+ のエージェントに対応。skills モード（`--integration-options="--skills"`）ではスラッシュコマンドの代わりに agent skills をインストール。Codex CLI は `$speckit-*`。
- **Kagi への示唆**: `.specify/` `.claude/` `.agents/` のような **「エージェント状態がリポジトリに commit される」**流れが確立した。Kagi の diff/ファイルツリーで **これらを「agent artifacts」として折りたたむ or 専用グループ化**すると、人間のレビューノイズが激減する（3.-#4）。
- **難易度**: S

#### plan mode / TDD with agents
- **出典**: https://code.claude.com/docs/en/common-workflows （`claude --permission-mode plan`、`Shift+Tab`、status bar に `⏸ plan mode on`）
- **仕組み**: Claude はファイルを読んで計画を提示するが、**承認まで disk に触らない**。ACP にも `session/set_mode` があり「agent operating modes」の切替が protocol レベルで定義されている（https://agentclientprotocol.com/protocol/v1/overview）。
- **Kagi への示唆**: **Kagi の `plan → confirm` はエージェント業界の "plan mode" と完全に同じ思想**。これは Kagi のマーケティング上の強い足場になる（「Kagi は 2026 年より前から plan mode を持っていた git クライアント」）。MCP tool として出すときも `kagi_plan_*` / `kagi_confirm_*` の 2 段に分けるのが業界慣習に合う（3.-#1）。
- **難易度**: —

#### MCP と git
- **何か**: git 操作を提供する MCP server の現状。
- **出典**:
  - 公式リファレンス実装: https://raw.githubusercontent.com/modelcontextprotocol/servers/main/src/git/README.md
  - GitHub MCP server（remote）: https://raw.githubusercontent.com/github/github-mcp-server/main/docs/remote-server.md
  - MCP tools 仕様: https://modelcontextprotocol.io/specification/2025-06-18/server/tools
  - MCP schema（annotation フィールド名）: https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/schema/2025-06-18/schema.ts (L891-923)
- **`mcp-server-git` の全ツール（12 個）**:
  | tool | 分類 | 引数 |
  |---|---|---|
  | `git_status` | 読み | `repo_path` |
  | `git_diff_unstaged` | 読み | `repo_path`, `context_lines`(既定3) |
  | `git_diff_staged` | 読み | `repo_path`, `context_lines` |
  | `git_diff` | 読み | `repo_path`, `target`, `context_lines` |
  | `git_log` | 読み | `repo_path`, `max_count`(既定10), `start_timestamp`, `end_timestamp` |
  | `git_show` | 読み | `repo_path`, `revision` |
  | `git_branch` | 読み | `repo_path`, `branch_type`(local/remote/all), `contains`, `not_contains` |
  | `git_add` | 書き | `repo_path`, `files[]` |
  | `git_commit` | 書き | `repo_path`, `message` |
  | **`git_reset`** | **書き（unstage のみ）** | `repo_path` |
  | `git_create_branch` | 書き | `repo_path`, `branch_name`, `base_branch` |
  | `git_checkout` | 書き | `repo_path`, `branch_name` |
  - **破壊的操作の評価**: `git_reset` は README 上「Unstages all staged changes」= `git reset`（mixed）相当で **`--hard` ではない**。`push --force` / `git clean` / `reset --hard` に相当するツールは**存在しない**。一方で **worktree / stash / rebase / merge / conflict 解決 / cherry-pick / revert / tag / remote 操作も一切無い**。README 自身が "currently in early development" と明記。MCP Python SDK 1.x (`mcp>=1.29.0,<2`) 必須で v2 移植は進行中。
- **GitHub MCP server（remote, `https://api.githubcopilot.com/mcp/`）の toolset 構成**:
  - toolset 単位で URL を切り替える設計: `/x/<toolset>`、`/x/all`、`/readonly` サフィックス、`/insiders`。
  - toolsets: `default`, `all`, `actions`, `code_quality`, `code_security`, `copilot`, `copilot_issue_intents`, `dependabot`, `discussions`, `gists`, **`git`（GitHub Git API による低レベル git 操作）**, `issues`, `labels`, `notifications`, `orgs`, `projects`, **`pull_requests`**, `repos`, `secret_protection`, `security_advisories`, `stargazers`, `users`。remote 限定で `copilot_spaces`, `github_support_docs_search`。
  - ヘッダによる制御: `X-MCP-Toolsets`, `X-MCP-Tools`, **`X-MCP-Readonly`**, **`X-MCP-Lockdown`**（push 権限の無いユーザが作った public issue の詳細を隠す。「best-effort content filter, not a security boundary」と明記）, `X-MCP-Insiders`。ローカル server の env var / flag と等価（`GITHUB_TOOLSETS`, `GITHUB_TOOLS`, `GITHUB_READ_ONLY`, `GITHUB_LOCKDOWN_MODE`）。
  - remote 限定ツールとして `create_pull_request_with_copilot`（Copilot coding agent を起動する）。
  - **設計上の学び**: 「read-only モードを URL/ヘッダで一発で切れる」「toolset を細かく分けて必要な分だけ有効化する」が MCP server 設計のベストプラクティスになっている。
- **MCP tool annotations（正確なフィールド名、schema.ts より）**:
  | フィールド | 型 | 既定 | 意味 |
  |---|---|---|---|
  | `readOnlyHint` | boolean | (未設定) | true なら環境を変更しない |
  | `destructiveHint` | boolean | **true** | true なら破壊的な更新をしうる。**`readOnlyHint == false` のときのみ意味を持つ** |
  | `idempotentHint` | boolean | **false** | 同じ引数での繰り返し呼び出しに追加効果が無い。**`readOnlyHint == false` のときのみ意味を持つ** |
  | `openWorldHint` | boolean | (未設定) | 外部エンティティの「open world」と相互作用しうる |
  - Tool 定義は `name` / `title`（表示用、任意）/ `description` / `inputSchema` / `outputSchema`（任意）/ `annotations`。
  - 結果は `content`（非構造）と **`structuredContent`（JSON オブジェクト）**。`outputSchema` があれば server は MUST 準拠、client は SHOULD 検証。後方互換のため構造化結果は TextContent にも JSON 文字列として入れるべき。
  - **仕様の警告 2 点**: (1)「For trust & safety and security, there SHOULD always be a human in the loop with the ability to deny tool invocations」「Applications SHOULD: どのツールが AI に露出しているかを明示する UI / ツール呼び出し時の視覚的インジケータ / 確認プロンプト」。(2)「clients MUST consider tool annotations to be untrusted unless they come from trusted servers」。
  - エラーは 2 系統: プロトコルエラー（JSON-RPC `error`）と実行エラー（結果に `isError: true`）。
- **ローカル git クライアントが MCP server になる意義**:
  - **既存 MCP git server の穴が Kagi の実装済み機能と正確に重なる**: worktree、stash、conflict 解決、oplog/undo、preflight、force-with-lease、squash-merge 検出。
  - **Codex は `.git` を read-only 保護する**（前述）。つまりエージェントは `.git` を直接叩けず、承認済みのコマンドか MCP tool 経由になる。**「安全な git 操作を tool として提供する」役が制度的に必要になっている。**
  - MCP annotation を正しく付けるだけで、Codex/Claude Code 側の承認 UI が Kagi の二段確認と連動する。
- **Kagi への示唆**: 3 章 #1, #10〜#15 を参照。
- **難易度**: L（新しい面の追加）

#### エージェントの作業を人間がレビューする UX
- **出典**: https://code.claude.com/docs/en/agent-view , https://github.com/BloopAI/vibe-kanban , https://github.com/dagger/container-use
- **仕組み**:
  - **Claude Code agent view**: 状態でグルーピング（`Ready for review` / `Needs input` / `Working` / `Completed`）。行サマリは **Haiku クラスのモデルが生成**（走行中は 15 秒ごとにモデル呼び出しなしで自前出力から更新、ターン終了時にモデルが書き直す）。行アイコンの**色 = 状態**、**形 = プロセス生存**（`✻`/`✽` 生存、`∙` 終了済みだが返信で再開、`✢` `/loop` の sleep 中）。フィルタ構文 `a:<name>` `s:<state>` `s:blocked` `#<number>`。`--cwd` でプロジェクト絞り込み。
  - **vibe-kanban**: diff に**インラインコメント**を書いてエージェントに直接フィードバック（UI を離れずに）。
  - **container-use**: 「エージェントの自己申告ではなく実際のコマンド履歴とログ」を見せる（`log --patch`, `watch`）。
- **Kagi への示唆**: Kagi は既に **repo タブ / worktree / diff split / PR conversation** を持つ。足りないのは「複数のエージェント作業を 1 画面で状態別に並べる」ビューと「diff にコメントを書いてエージェントに戻す」経路。前者は worktree パネルの拡張、後者は PR review conversation 機構の再利用でいける。
- **難易度**: M

---

### 2.3 AI native アプリ側の設計論

#### 「AI が操作できる GUI アプリ」の作り方 — 先行パターン
- **Zed / ACP (Agent Client Protocol)**
  - **出典**: https://agentclientprotocol.com/protocol/v1/overview
  - JSON-RPC 2.0。**Agent 側**: `initialize`, `authenticate`, `session/new`, `session/prompt`（baseline）、`session/load`, `logout`, `session/set_mode`（optional）、通知 `session/cancel`。**Client（= エディタ/GUI）側**: `session/request_permission`（baseline!）、`fs/read_text_file`, `fs/write_text_file`, `terminal/create|output|release|wait_for_exit|kill`, `elicitation/create`（optional）、通知 `session/update`, `elicitation/complete`。
  - 規約: **全ファイルパスは絶対パス MUST**、行番号は 1-based。プロパティキーは `camelCase`、discriminator の文字列値は `snake_case`。拡張は `_meta` フィールドと `_` プレフィックスのカスタムメソッド。
  - **`session/request_permission` が baseline（必須）である**のが設計上の要点 — 「エージェントがツールを使うには client の承認を得る」がプロトコルの前提になっている。
  - **Kagi への示唆**: **Kagi は MCP server（= agent に tool を出す側）と ACP client（= agent をホストする側）の両方になれる。** 前者が本命だが、後者（Kagi 内にエージェントペインを持つ）も筋は通る。`session/request_permission` は Kagi の confirm モーダルにそのまま写像できる。
- **container-use**: `container-use stdio` で MCP server 化。CLI と MCP server が**同一バイナリ**。
- **vibe-kanban**: `MCP_HOST` / `MCP_PORT` 環境変数を持ち、バックエンドが MCP server を兼ねる。
- **Kagi の既存資産との対比**: Kagi は既に **Unix domain socket による single-instance IPC** を持つ（`src/single_instance.rs`）。`/tmp/kagi-instance-<user>.sock` に「1行 = 正規化された絶対リポジトリパス、空行 = フォーカスのみ」を書く極小プロトコル。**これを JSON-RPC に拡張すれば MCP server 面の下地になる**（3.-#10）。

#### エージェントに危険な操作をさせないためのガードレール設計（Kagi の plan/preflight と対比）
| レイヤ | 業界の実装 | Kagi の既存実装 | ギャップ |
|---|---|---|---|
| ツール宣言レベル | MCP `readOnlyHint` / `destructiveHint` / `idempotentHint` / `openWorldHint`。Codex は destructive annotation を持つ tool call を必ず承認要求 | 「破壊的操作はコードベースに存在しない」= 宣言以前に**不在** | Kagi が MCP tool を出すとき annotation を付ける作業が必要 |
| 承認レベル | ACP `session/request_permission`（baseline）、MCP 仕様の「human in the loop SHOULD」、Codex の `--ask-for-approval untrusted/on-request/never`、granular policy | `plan → confirm`、破壊的操作は二段確認 | MCP/headless 経路には confirm UI が無い → **誰が confirm するのか**の設計が必要 |
| 実行前検証 | Codex sandbox（`.git` read-only、network 既定オフ、domain allowlist、DNS rebinding チェック）。Claude Code の worktree 4 チェック（file edits / cwd / git redirects / command shape） | `preflight_check`（plan 以降 HEAD/stash-count が変わっていたら拒否）、`preflight_worktree_digest`（ADR-0147） | Kagi の preflight は「plan の陳腐化」検出。**エージェント特有の TOCTOU（別エージェントが同時に触る）**は worktree digest でカバーされるか要検証 |
| 実行後 | Codex の auto-review（reviewer agent が data exfiltration / credential probing / destructive actions を審査、critical は deny、parse 失敗は fail closed） | `verify_*` + oplog + undo/redo + discard 時の ODB blob バックアップ | **`Backend::run` は oplog を書かない**（ADR-0104: 「oplog/toast/footer recording stays with the UI's `record_op`」）→ ヘッドレス面の致命的な穴 |
| 監査 | container-use の実コマンド履歴、Amp の `Amp-Thread-ID:` trailer、Copilot cloud agent の「every step happening in a commit and viewable in logs」 | oplog（`$KAGI_LOG_DIR/operations.jsonl` または `~/.kagi/operations.jsonl`、JSONL、手書き serialize、`read_oplog_tail(n)`） | oplog は**グローバル 1 ファイル**でリポジトリ別ではない。エージェント別の attribution フィールドも無い |

#### ローカルモデルでのコミットメッセージ / 差分要約
- **opencommit (`oco`)**
  - **出典**: https://github.com/di-sukharev/opencommit
  - `git add <files>` → `oco`（`git add` 自体も `oco` が代行）。プロバイダは `OCO_AI_PROVIDER` で切替: `openai`(既定), `anthropic`, `azure`, **`ollama`**, **`llamacpp`**, `gemini`, `flowise`, `deepseek`, `aimlapi`, `openrouter`, `orcarouter`。
  - Ollama: `oco config set OCO_AI_PROVIDER='ollama' OCO_MODEL='llama3:8b'`（既定モデルは `mistral`）。エンドポイントは `OCO_API_URL='http://192.168.1.10:11434/api/chat'`。IPv6 問題の回避に `export OLLAMA_HOST=0.0.0.0`。
  - llama.cpp: `./llama-server -m model.gguf --port 8080` → `OCO_AI_PROVIDER='llamacpp' OCO_API_URL='http://localhost:8080'`。
  - 出力制御: `OCO_PROMPT_MODULE`（`conventional-commit`(既定) / `@commitlint`）、`OCO_EMOJI`（10 個サブセット）/ `--fgm`（full gitmoji）、`OCO_EMOJI_POSITION_BEFORE_DESCRIPTION`（`fix(server.ts): 🐛 ...`）、`OCO_ONE_LINE_COMMIT`、`OCO_LANGUAGE`、`OCO_TOKENS_MAX_INPUT`（既定 4096）/ `OCO_TOKENS_MAX_OUTPUT`（既定 500）、`OCO_REASONING` / `OCO_REASONING_MAX_TOKENS`（既定 1000）、`OCO_DESCRIPTION`、`OCO_WHY`(WIP)。`oco --yes` で確認スキップ。
  - リポジトリ単位設定は `.env`、グローバルは `~/.opencommit`。**ローカル優先**。`oco models` / `oco models --refresh`（モデル一覧は 7 日キャッシュ）。
- **Kagi の既存実装との比較（`crates/kagi-git/src/message_gen.rs` を読んだ上で）**
  | 観点 | opencommit | Kagi 現状 |
  |---|---|---|
  | プロバイダ | 11 種（クラウド + Ollama + llama.cpp） | Ollama（`/api/generate`、既定 `localhost:11434`、`KAGI_OLLAMA_HOST` で上書き、HTTP timeout 45s）+ **agentic CLI**（`claude -p --output-format text` / `codex exec -s read-only --color never -o <TMPFILE> -`、timeout 60s、ADR-0099）+ `rule_based`（infallible） |
  | diff の渡し方 | `git diff --cached`、`OCO_TOKENS_MAX_INPUT` で制限 | **staged only**（`collect_staged_diff` は HEAD tree vs index）、`DIFF_TRUNCATE_BYTES` で行境界切り + `[... truncated ...]` + ファイルサマリ追記 |
  | 出力形式 | conventional-commit / commitlint / gitmoji | lang/style aware なプロンプト（`Lang` / `build_prompt`） |
  | オフライン | — | **`KAGI_OFFLINE=1` で検出も生成も完全停止**（staged diff がマシンを出ない保証） |
  | LM Studio | 記載なし | 未対応 |
  - **Kagi は既に opencommit より安全側の設計**（staged only、offline switch、CLI は read-only モード強制、`--bare` を意図的に渡さない）。足りないのは **LM Studio / OpenAI 互換エンドポイント**（LM Studio は `/v1/chat/completions` を出す）と **Conventional Commits の構造化出力**。
- **`prepare-commit-msg` フック統合**: opencommit はフック統合も持つ（README 後半）。**Kagi への示唆**: Kagi が GUI で生成したメッセージと、リポジトリの `prepare-commit-msg` フックが競合しうる。Kagi 側でフックの存在を検出して警告する価値がある。
- **難易度**: LM Studio 対応 S / Conventional Commits 構造化 M

---

## 3. Kagi 取り込み候補（優先順）

| # | 提案 | 効果 | 難易度 | 依存 | 出典 |
|---|---|---|---|---|---|
| 1 | **`kagi-mcp` — Kagi を MCP server 化する**（下に詳細設計）。`Backend` を stdio JSON-RPC で公開。read tool は無条件、write tool は `plan` → `confirm` の 2 段。annotation を正しく付ける | 「破壊的操作が存在しない git MCP server」は空席。エージェントが Kagi 経由で git を触れば、force push も reset --hard も**構造的に不可能**になる | L | #10, #11, #13 | https://raw.githubusercontent.com/modelcontextprotocol/servers/main/src/git/README.md , https://modelcontextprotocol.io/specification/2025-06-18/server/tools |
| 2 | **コミットグラフに「エージェント来歴」列/バッジ**。trailer（`Co-Authored-By`, `Amp-Thread-ID`, 任意の `X-*`）+ author/committer の 3 経路から判定 | 他のどの GUI にも無い。エージェント時代の履歴は「誰が書いたか」が author では分からない | S | trailer パーサ（#3） | https://ampcode.com/docs/markdown/github , https://code.claude.com/docs/en/settings-reference , https://jules.google/docs/changelog/ |
| 3 | **git trailer パーサ + コミット詳細ペインでの trailer 表示（URL はリンク化）**。`git interpret-trailers` の検出ルール（全行 trailer、または git生成/設定済み trailer を1つ以上含み 25% 以上が trailer、かつ空行前置）に準拠 | `Amp-Thread-ID` をクリックして会話に飛べる。`BREAKING CHANGE:` / `Refs:` / `Signed-off-by:` も同時に恩恵 | S | — | https://git-scm.com/docs/git-interpret-trailers (2.54.0) |
| 4 | **「agent artifacts」ファイル分類**。`AGENTS.md`, `CLAUDE.md`, `.claude/**`, `.cursor/rules/**`, `.github/copilot-instructions.md`, `.specify/**`, `.agents/**`, `.worktreeinclude` を diff/ツリーで専用グループ化（既定で折りたたみ、規約ファイル変更には強調バッジ） | エージェント状態が commit される流れが確立。人間のレビューノイズが減り、規約変更は逆に目立つ | S | — | https://agents.md/ , https://github.com/github/spec-kit , https://code.claude.com/docs/en/worktrees |
| 5 | **worktree パネルを「エージェントセッション一覧」に拡張**。`.claude/worktrees/*` / `cu-*` ブランチ / `worktree-*` ブランチを自動分類。`git worktree lock` 状態、HEAD、base からの ahead/behind、dirty 有無、last-commit 時刻を1行で | 並列エージェント運用の主戦場。Claude Code agent view / claude-squad / container-use が全部これを持っている | M | worktree 管理（既存） | https://code.claude.com/docs/en/agent-view , https://github.com/smtg-ai/claude-squad , https://raw.githubusercontent.com/dagger/container-use/main/docs/cli-reference.mdx |
| 6 | **複数 worktree/ブランチの diff 横並び比較 → 1 つを選んで統合**。`container-use merge`（履歴保持）と `container-use apply`（staged 変更として適用、コミットせず）の 2 モードを再現 | 「同じタスクを 3 エージェントに投げて良いのを選ぶ」パターンに直接効く。Kagi の split diff を再利用できる | L | #5 | https://raw.githubusercontent.com/dagger/container-use/main/docs/cli-reference.mdx |
| 7 | **「エージェント試行の shadow commit / auto-snapshot」**。作業ツリーの状態を通常の ref 名前空間の外（`refs/kagi/snapshots/<ts>`）に dangling commit として記録し、タイムラインから復元可能に。ODB blob バックアップ機構の一般化 | Claude Code / Cursor の checkpoint が「git ではない」と明言している空白を埋める。bash 由来の変更も subagent の編集も捕まえられる（checkpoint は捕まえられない） | M | discard の ODB backup（既存） | https://code.claude.com/docs/en/checkpointing , https://cursor.com/docs/agent/overview |
| 8 | **oplog を「エージェント別」に拡張**。`OpLogEntry` に `actor`（human / mcp:<client-name> / cli）と `session`（会話 URL/ID）を追加。リポジトリ別分割も検討（現状 `~/.kagi/operations.jsonl` の 1 ファイル） | MCP 面を作った瞬間に「誰がやったか」が必須になる。監査と undo の粒度が上がる | M | #1, oplog v2（ADR-0074） | 内部: `crates/kagi-git/src/oplog.rs`; 対比: https://github.com/dagger/container-use |
| 9 | **mergiraf をオプトイン前処理として統合**。conflict editor を開く前に `mergiraf solve <file>` を提案（プレビュー付き）。`mergiraf review <merge-id>` の代わりに Kagi の conflict editor で hunk 単位レビュー | mergiraf の弱点は「自動解決したがレビューが必要」状態の UI。Kagi はそれを既に持っている。相互補完 | M | conflict editor（既存）、diff3（既存） | https://mergiraf.org/usage.html |
| 10 | **single-instance socket を JSON-RPC に格上げ**。現状の「1行 = パス」プロトコルを `{"jsonrpc":"2.0",...}` に拡張（先頭文字で旧形式と判別して後方互換）。GUI が起きていれば MCP server も socket 越しに動く | MCP server 面の土台。GUI と headless の二重実装を避けられる | M | — | 内部: `src/single_instance.rs`; 対比: https://raw.githubusercontent.com/dagger/container-use/main/docs/cli-reference.mdx (`container-use stdio`) |
| 11 | **`Backend::run` に oplog 記録を移す**（ADR-0104 が残した穴）。`record_op` の UI 依存部分（toast/footer）と oplog 部分を分離し、oplog は `run` の中で必ず書く | **これを直さないと MCP/ヘッドレス経路の書き込みが oplog に載らず、「undo できる」という製品の約束が崩れる。#1 の前提条件** | M | — | 内部: `docs/adr/0104-enforced-operation-pipeline.md` Consequences |
| 12 | **`kagi` CLI サブコマンド（ヘッドレス面の正規化）**。`kagi plan <op> [args] --json` / `kagi confirm <plan-id> --json` / `kagi status --json` / `kagi oplog --json`。`KAGI_*` 環境変数ハーネスは**テスト専用のまま残し**、CLI とは別の面にする | エージェントは CLI から呼べる方が導入障壁が低い。MCP server は CLI の薄いラッパになる。`claude -p` / `codex exec` から直接使える | M | #11, #13 | https://code.claude.com/docs/en/headless , https://ampcode.com/docs/markdown/cli/execute-mode |
| 13 | **`OperationPlan` に安定 ID と JSON シリアライズを追加**。plan を返す → 別プロセス/別ターンで confirm する、という 2 段を成立させる。plan には `preflight` の前提（HEAD sha, stash count, worktree digest）を含める | plan/confirm を跨プロセスで成立させる唯一の方法。preflight が既に「plan の陳腐化」を検出するので、ID 化すれば TOCTOU 保護がそのまま効く | M | — | 内部: ADR-0104, ADR-0147; 対比: https://agentclientprotocol.com/protocol/v1/overview (`session/request_permission`) |
| 14 | **MCP tool annotation を Kagi の危険度分類に写像**。read tool → `readOnlyHint: true`、branch 作成/stash push → `destructiveHint: false, idempotentHint: false`、discard/delete-branch/reset(soft/mixed) → `destructiveHint: true`、fetch/push → `openWorldHint: true` | Codex は「destructive annotation を持つ tool は必ず承認要求」を実装済み。annotation を付けるだけでエージェント側のガードレールに乗れる | S | #1 | https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/schema/2025-06-18/schema.ts L891-923 , https://learn.chatgpt.com/docs/agent-approvals-security |
| 15 | **read-only モードのワンスイッチ**。`kagi-mcp --readonly` / 環境変数 / GUI の設定トグルで write tool を `tools/list` から消す（`listChanged` 通知付き） | GitHub MCP server の `/readonly` URL・`X-MCP-Readonly` ヘッダが標準になっている。「まず read-only で試す」導線が採用率を上げる | S | #1 | https://raw.githubusercontent.com/github/github-mcp-server/main/docs/remote-server.md |
| 16 | **PR 一覧に「エージェント作成 PR」バッジ + CI 状態の色分け**。Copilot cloud agent / Jules / Amp が開いた PR を author から判定。Claude Code agent view の色規約（黄=checks/review待ち、緑=通過、紫=merged、灰=draft/closed）を借用 | Kagi は既に PR 一覧・PR merge・conflict preview を持つ。エージェント PR は数が増えるので分類が要る | S | GitHub 連携（既存） | https://code.claude.com/docs/en/agent-view , https://docs.github.com/en/copilot/concepts/agents/cloud-agent/about-cloud-agent |
| 17 | **diff にインラインコメントを書いてエージェントに送る**。PR review conversation の仕組みをローカル diff に流用。出力は Markdown（`path:line — comment`）でクリップボード or `local://` ファイル | vibe-kanban の中核機能。「レビューが律速」というエージェント時代の本質に直撃 | M | PR conversation（既存） | https://github.com/BloopAI/vibe-kanban |
| 18 | **Conventional Commits の構造化サポート**。commit panel で type/scope/`!`/footer を別フィールドに。グラフ上で `!` と `BREAKING CHANGE:` にバッジ。`revert:` + `Refs: <sha>` を revert 操作が自動生成 | 仕様 v1.0.0 が安定し、opencommit 等が既定で従う。AI 生成メッセージの検証にも使える | M | #3（footer = trailer） | https://www.conventionalcommits.org/en/v1.0.0/ |
| 19 | **smart commit に LM Studio / OpenAI 互換エンドポイント対応を追加**。`message_gen.rs` の Ollama パスの隣に `/v1/chat/completions` パスを足す。`KAGI_OFFLINE` と staged-only は維持 | opencommit は 11 プロバイダ。Kagi は Ollama + agentic CLI のみ。LM Studio ユーザを取り込める | S | 既存 `message_gen` | https://github.com/di-sukharev/opencommit |
| 20 | **`.worktreeinclude` / `.gitignore` 済みファイルの worktree 間コピー支援**。Kagi の worktree 作成時に `.worktreeinclude` を読んで gitignore 済みファイルのみコピー（tracked は複製しない） | Claude Code が既に持つ機能。Kagi で worktree を作ったときに `.env` が無くてビルドが落ちる、という実害を消す | S | worktree 作成（既存） | https://code.claude.com/docs/en/worktrees |
| 21 | **`prepare-commit-msg` フック検出と警告**。Kagi の smart commit が生成したメッセージがフックに上書きされる可能性を事前に警告 | opencommit 等のフック統合と Kagi GUI の競合を防ぐ | S | — | https://github.com/di-sukharev/opencommit , https://git-scm.com/docs/githooks |
| 22 | **fetch-age indicator の重み付けを上げる**。Amp の既定 Ship 挙動は trunk-based で `origin/main` に直 push。エージェント時代は base branch が高頻度で動く | Kagi は既に fetch age indicator（ADR-0127）と force-with-lease を持つ。閾値の調整と、stale な base への操作への警告強化 | S | ADR-0127（既存） | https://ampcode.com/docs/markdown/orbs/shipping |

### #1 の詳細設計 — `kagi-mcp` のツール一覧（シグネチャ案）

新クレート `crates/kagi-mcp/`（または `src/bin/kagi-mcp.rs`）。`kagi-git::Backend` にのみ依存し、`gpui` には依存しない（AGENTS.md の依存方向 `kagi(bin) → ui + git + kagi-domain` に反しない。`kagi-mcp → kagi-git → kagi-domain`）。

**トランスポート**: stdio JSON-RPC（`kagi-mcp stdio`）。GUI が起きている場合は `/tmp/kagi-instance-<user>.sock` に転送して GUI に confirm モーダルを出させる（#10）。GUI が無ければ CLI 引数/設定で承認ポリシーを決める。

**Read tools（すべて `readOnlyHint: true`）**

```
kagi_repo_status(repo_path: string)
  -> { branch, head_sha, upstream, ahead, behind, dirty: bool,
       staged: [FileStatus], unstaged: [FileStatus], untracked: [string],
       conflicts: [string], stash_count: int, fetch_age_secs: int|null }

kagi_graph(repo_path: string, limit?: int = 200, branch?: string)
  -> { rows: [{ sha, lane, parents: [sha], refs: [string], summary,
                author, author_email, committer, timestamp,
                trailers: { [key]: [value] },        # ← 来歴の核心
                is_stash: bool, ghost_merge_of?: sha }] }   # squash-merge 検出

kagi_diff(repo_path, from?: rev, to?: rev, paths?: [string],
          staged?: bool, context_lines?: int = 3, max_bytes?: int)
  -> { files: [{ path, old_path?, status, hunks: [...], additions, deletions }],
       truncated: bool }

kagi_commit_show(repo_path, revision: string)
  -> { sha, message, subject, body, trailers, parents, diffstat, files }

kagi_branches(repo_path, kind?: "local"|"remote"|"all",
              contains?: sha, merged_into?: rev)
  -> { branches: [{ name, sha, upstream?, ahead, behind,
                    is_head, squash_merged_into?: string }] }

kagi_worktrees(repo_path)
  -> { worktrees: [{ path, branch, head_sha, locked: bool, lock_reason?,
                     dirty: bool, ahead, behind,
                     agent_hint?: "claude"|"container-use"|null }] }   # #5 と共有

kagi_conflicts(repo_path)
  -> { in_progress: "merge"|"rebase"|"cherry-pick"|"revert"|"none",
       files: [{ path, groups: [{ id, ours, theirs, base, resolved? }] }] }

kagi_stashes(repo_path)
  -> { stashes: [{ index, sha, message, branch, timestamp }] }

kagi_oplog(repo_path?, limit?: int = 50)
  -> { entries: [OpLogEntry] }      # read_oplog_tail を再利用

kagi_file_history(repo_path, path: string, limit?: int = 50)
  -> { commits: [{ sha, summary, timestamp, additions, deletions, renamed_from? }] }

kagi_analyze(repo_path, kind: "hotspots"|"coupling"|"ownership", limit?: int)
  -> { ... }      # 既存 Analyze 機能の JSON 化。ownership に AI/human 軸を足せる（#2）
```

**Plan tools（`readOnlyHint: true` — plan は何も変えない）**

```
kagi_plan(repo_path, op: OperationSpec)
  -> { plan_id: string,               # 安定 ID（#13）
       plan_note: string,             # 既存の plan_note（i18n 済み、人間向け）
       op: OperationSpec,
       before: StateSummary,          # head_sha / dirty / stash_count / worktree_digest
       predicted_after: StateSummary,
       risk: "safe"|"confirm"|"double_confirm",
       blockers: [string],            # preflight が今すぐ拒否する理由（dirty tree など）
       expires_when: { head_sha, stash_count, worktree_digest } }
```

`OperationSpec` は既存 `Operation` enum の JSON 表現。tagged union:
`{"kind":"checkout","target":"feature/x"}` / `{"kind":"merge","from":"feature/x","strategy":"ff_only"}` / `{"kind":"discard","paths":["a.rs"]}` など。

**Write tools（すべて `plan_id` を必須にする = plan なしに書き込めない）**

| tool | シグネチャ | `readOnlyHint` | `destructiveHint` | `idempotentHint` | `openWorldHint` |
|---|---|---|---|---|---|
| `kagi_confirm` | `(repo_path, plan_id, acknowledge_risk?: bool)` → `{ outcome, after: StateSummary, oplog_id }` | false | **plan の `risk` から動的に決定**（`double_confirm` → true） | false | op 次第 |
| `kagi_stage` | `(repo_path, paths: [string])` | false | false | **true** | false |
| `kagi_unstage` | `(repo_path, paths: [string])` | false | false | **true** | false |
| `kagi_commit` | `(repo_path, plan_id, message: string, trailers?: {[k]:string})` | false | false | false | false |
| `kagi_resolve_conflict` | `(repo_path, path, group_id, choice: "ours"\|"theirs"\|"manual", content?)` | false | false | false | false |
| `kagi_undo` | `(repo_path, oplog_id?)` → oplog の直前操作を戻す | false | false | false | false |
| `kagi_worktree_create` | `(repo_path, plan_id, name, base?: rev, branch?: string)` | false | false | false | false |
| `kagi_fetch` | `(repo_path, remote?: string)` | false | false | true | **true** |
| `kagi_push` | `(repo_path, plan_id, remote?, refspec?, with_lease: bool = true)` | false | false | false | **true** |

**意図的に提供しないツール（これが製品の主張）**

`kagi_reset_hard` / `kagi_push_force` / `kagi_clean` は**存在しない**。tool description に明記する:
> Kagi never exposes `reset --hard`, `push --force`, or `git clean`. Force-push is available only as `--force-with-lease` via `kagi_push(with_lease: true)`. To recover from a bad state, use `kagi_oplog` + `kagi_undo`.

これは MCP の `tools/list` に現れる**唯一の「安全性の宣言」**になる。エージェントは tool 一覧を見て「force push できない環境だ」と理解して行動を変える。

**`outputSchema` を全 tool に付ける**（MCP 2025-06-18 仕様の `outputSchema` + `structuredContent`）。後方互換のため JSON を TextContent にも入れる。

**read-only モード**（#15）: `kagi-mcp stdio --readonly` で write/plan tool を `tools/list` から除外し、`notifications/tools/list_changed` を送る。

---

## 4. 取り込まないと判断したもの（理由付き）

| 項目 | 理由 |
|---|---|
| **Kagi 内にエージェント実行環境（コンテナ / VM）を持つ** | container-use / Amp orbs / Copilot cloud agent が既に激戦区で、Kagi の強み（git の安全性）と関係が無い。Docker/Dagger への依存も Kagi のインストール体験（macOS/Linux ネイティブ、単一バイナリ）を壊す。**worktree 分離までで止めるのが正しい**。 |
| **Kagi を ACP client にしてエージェントペインを内蔵する** | 筋は通るが、Zed が本命の実装を持っており、Kagi は既に埋め込みターミナルを持つ（`claude` / `codex` をそこで動かせる）。MCP server 面（tool を出す側）の方が ROI が高い。**後回し**。 |
| **checkpoint 機構を Cursor/Claude Code と同じ形（git 外のスナップショット）で作る** | git クライアントが git 外の履歴を持つのは自己矛盾。#7 の「dangling commit を `refs/kagi/snapshots/` に置く」形なら git の世界に留まる。**同じ問題を git の作法で解く**。 |
| **`git reset` 系ツールを MCP に出す（mixed/soft も含めて）** | `mcp-server-git` の `git_reset` は unstage 相当で危険度は低いが、Kagi は `kagi_unstage(paths)` として**意図が明確な名前**で出すべき。`reset` という語を tool 名に出さないこと自体がガードレール。 |
| **AI にコンフリクト解決を丸投げする機能（AI merge driver を Kagi が内蔵）** | mergiraf は**構文ベースで決定論的**（LLM ではない）ので統合価値が高い（#9）。一方 LLM ベースの自動 merge は「間違いを静かに埋め込む」リスクが Kagi の存在理由と正面衝突する。**mergiraf は入れる、LLM merge は入れない。** |
| **gitmoji の全面サポート** | opencommit が既定で 10 個サブセットに絞っている程度の生存度。Conventional Commits（#18）の方が投資対効果が高い。絵文字の描画は既に Kagi でできる（表示だけで十分）。 |
| **Copilot cloud agent / Jules / Devin をトリガーする機能（Kagi から起動）** | remote GitHub MCP server の `create_pull_request_with_copilot` などが既にある。Kagi が「エージェントを起動する UI」を持つとスコープが無限に広がる。**Kagi は「エージェントが作ったものを人間が理解する場所」に集中する。** |
| **oplog を git notes / refs に保存する** | 現状の JSONL（`operations.jsonl`）はリポジトリを汚さない良い設計。git に書くと push/fetch の話が発生し、preflight の前提も複雑になる。#8 の拡張（actor/session フィールド追加）だけで足りる。 |
| **`KAGI_*` 環境変数ハーネスをエージェント向け公開 API に昇格させる** | `src/headless.rs` 自身が「read-only UI-state hooks のみ、ADR-0077 で mutating hooks は撤去済み、`tests/` の統合テストと重複していたため」と書いている。**テスト契約として残し、エージェント向けには #12 の `kagi` CLI サブコマンドを別に作る**のが正しい。env var は 1 プロセス 1 操作しか表現できず、plan/confirm の 2 段に向かない。 |

---

## 5. 未解決の疑問

1. **MCP 経路の `confirm` は誰が承認するのか。** 3 案: (a) GUI が起きていれば socket 経由でモーダルを出す（人間が承認、Kagi の思想に最も忠実だが GUI 必須）、(b) MCP `destructiveHint` に任せてホスト側（Claude Code / Codex）の承認 UI を使う（実装は軽いが Kagi の plan_note が見えない）、(c) 両方（GUI があれば a、なければ b）。**(c) が正解に見えるが、a の場合に「エージェントがブロックされて 5 分待つ」体験をどう設計するか未解決。** ACP の `session/request_permission` と MCP の elicitation のどちらに寄せるかも要調査。
2. **`preflight_check` は「別エージェントの同時書き込み」を検出できるか。** 現状の preflight は「plan 以降 HEAD/stash-count が変わったら拒否」+ ADR-0147 の worktree digest。**同一 worktree に 2 エージェントが同時に MCP 経由で書いた場合の挙動を実測していない。** index.lock 競合の扱いも要検証。
3. **`Amp-Thread-ID` のような独自 trailer をどこまで知識として持つか。** ホワイトリスト方式（既知のエージェント trailer を認識）か、汎用方式（全 trailer を表示し URL だけリンク化）か。汎用の方が保守が楽だが、「エージェント由来」の判定精度は落ちる。**`Co-Authored-By` のメールドメイン（`noreply@anthropic.com`, `amp@ampcode.com`）で判定するのが現実的か？** Jules の「人間単独 author」モードは原理的に検出不可能。
4. **`.claude/worktrees/` を Kagi が掃除してよいか。** Claude Code は worktree の git metadata にマーカーを書き、マーカー無しの worktree を sweep から除外する（v2.1.246+）。**Kagi が worktree を削除するとき、Claude Code の lock とマーカーをどう尊重するか。** 逆に Kagi が作った worktree に Kagi 独自マーカーを書くべきか。
5. **oplog をリポジトリ別に分割すべきか。** 現状 `~/.kagi/operations.jsonl` の 1 ファイル。MCP 経路で複数リポジトリが並列に書くと 1 ファイルへの追記競合が起きる。**append-only で行単位なら O_APPEND で安全か、それともリポジトリ別に分けるか。** ADR-0074（oplog format v2）が何を決めているか未確認。
6. **`Backend::run` に oplog を移す際、既存の `record_op`（toast/footer/undo stack）との責務分割をどう切るか。** ADR-0104 は「oplog/toast/footer recording stays with the UI's `record_op` because those are UI concerns」と書き、ADR-0073（worker thread consolidation）が「may fold them in later」としている。**#11 は ADR-0073 の一部として実施すべきか、独立した ADR にすべきか。**
7. **Kagi の `plan_note`（i18n 済みの人間向け説明文）を MCP tool のレスポンスに含めるべきか。** エージェントに読ませる価値はある（何が起きるかを理解させる）が、EN/JA どちらを返すか、`structuredContent` と `content` のどちらに置くかが未決。
8. **LM Studio の実際のエンドポイント形状を未検証。** `/v1/chat/completions`（OpenAI 互換）と `/api/v0/*`（LM Studio 独自）の両方があると理解しているが、2026-09 時点の一次情報で確認していない。**#19 の実装前に LM Studio 公式 docs を読む必要がある。**
9. **`mergiraf` の Rust クレートとしての統合可否。** CLI（`mergiraf merge` / `solve` / `review`）としての統合は確実にできるが、**ライブラリとして `kagi-git` に組み込めるか（crates.io に lib crate があるか、tree-sitter grammar のバイナリサイズ影響）は未調査**。Kagi は単一バイナリ配布なのでサイズは無視できない。
10. **エージェント PR の author 判定に使える安定した識別子は何か。** Copilot cloud agent は `app/copilot-swe-agent` か `Copilot` か（GitHub API のレスポンス実物を未確認）。Jules / Devin / Amp も同様。**GitHub API の `user.type == "Bot"` で十分か、それとも個別のホワイトリストが必要か。**
