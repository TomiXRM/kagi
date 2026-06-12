# ADR-0044: Smart Commit Message Generation

- Status: **Proposed**(既定バックエンドの確定がユーザー判断点 — 後述)/ Date: 2026-06-13

## Context

staged diff から commit message を自動生成する(v0.2)。ユーザー環境には ollama + gemma 等のローカル LLM が
ある。**設計が肝**: ローカル LLM を第一バックエンドにしつつ、外部送信を最小化し(ADR-0037)、LLM 不在でも
壊れない rule-based fallback を用意する。Conventional Commits / 日英切替に対応する。

## Decision

### バックエンド抽象 = 関数 enum dispatch(trait 過剰設計しない)

```rust
pub enum MessageBackend {
    Ollama { host: String, model: String },   // 既定候補。localhost:11434
    RuleBased,                                 // LLM 不在/失敗時の fallback
    // External { ... } は将来。デフォルト無効、ユーザーが明示設定した時のみ
}

pub struct GenInput { diff: String, lang: Lang, style: Style }  // style=ConventionalCommits 等
pub fn generate_message(backend: &MessageBackend, input: &GenInput) -> Result<String, GenError>;
```

- trait は作らない(YAGNI、avatar resolver と同方針)。`generate_message` 内で enum 分岐。
- 失敗時は `Err` を返し、呼び出し側が **rule-based fallback に落として静かに手動編集へ**(モーダルやエラー
  バナーで止めない)。

### 入力: staged diff のみ

- **staged diff のみを送る**(`unstaged` は含めない。要件の「staged から生成」「unstaged 含めない」に厳守)。
- diff が大きい場合は **先頭 N(例 ~8KB / 数百行)に truncate** + ファイル一覧サマリを添える(token / latency 対策)。
- diff に secret らしき内容(ADR-0043 の検出)が含まれていても **外部には出さない**(下記のとおり既定 local のみ)。

### ネットワーク方針(ADR-0037 の精神)

- **既定はローカルのみ**(ollama = localhost:11434、ループバック)。staged diff が外部ネットワークに出ることは
  既定で**ない**。
- **外部 API バックエンドはユーザーが明示設定した場合に限り**有効化(設定に endpoint + 同意フラグ。デフォルト無効)。
- `KAGI_OFFLINE=1` で LLM 呼び出しを完全停止 → 常に rule-based(headless テストは決定的に)。
- HTTP は **ureq 3 を再利用**(avatar_fetch.rs と同じ blocking GET/POST + global timeout)。新依存を足さない。
- 呼び出しは **background**(`cx.background_spawn`)。**タイムアウト**(例 グローバル数秒)。失敗/タイムアウトは
  静かに rule-based fallback または手動編集に戻す。

### ollama 呼び出し

- `POST http://<host>/api/generate`(または `/api/chat`)に `{ model, prompt, stream:false }`。
- prompt: 「以下の staged diff を要約し Conventional Commits 形式の commit message を生成。<lang>。本文は簡潔に。」
  + truncate した diff + ファイルサマリ。応答 JSON の `response` を取り出す(手書き JSON parse、serde 不要)。
- ストリーミング表示は MVP 外(`stream:false`)。

### rule-based fallback(LLM 不在時)

- staged の **ファイル種別・追加/削除・パス**から定型生成:
  - 単一ファイル・新規 → `feat: add <file>` / 削除 → `chore: remove <file>` / 変更 → `<type>: update <file>`
  - 複数ファイル → 主要ディレクトリ or ファイル数から `<type>(<scope>): update N files` 等
  - type 推定: テストのみ → `test`、docs のみ → `docs`、設定 → `chore`、それ以外 → `feat`/`fix` は控えめに `chore`/`feat`
- **必ず何か返す**(空にしない)。あくまで叩き台で、ユーザーが編集する前提。

### Conventional Commits / 日英

- `Style::ConventionalCommits`(既定)で `type(scope): summary` を促す。プレーンも可。
- `Lang::Ja | En` で prompt と rule-based の語彙を切替(UI トグル。既定はユーザーの選択を draft と同様に記憶)。

### 既定バックエンド(★ユーザー判断点 — Proposed)

- **案 A(ollama 既定で自動検出)**: 起動時に `localhost:11434` を軽く叩いて到達可能なら ollama を既定に、
  不可なら rule-based。設定ゼロで「ある人は LLM、無い人は定型」。**推奨**(備考の ollama 環境に合致)。
  - 要決定: 既定 model 名(`gemma` 等)をどう選ぶか。`/api/tags` で入っている model を列挙し先頭/設定値を使う。
- **案 B(既定 rule-based、LLM は明示 opt-in)**: 最も保守的。LLM を使う人だけ設定で有効化。
- **推奨**: A(到達確認つき自動)。ただし「常に localhost を叩く」ことの可否と既定 model 選択はユーザー決定。→ **要決定**。

## Consequences

- staged diff が外部に出るのは「ユーザーが external backend を明示設定」した時のみ。既定は loopback or ローカル計算
- trait なし・enum dispatch でバックエンド追加は分岐追加で済む(将来 external も同様)
- ureq 再利用で依存純度を保つ
- Proposed のため、実装チケット(T-COMMIT-015〜017)は**既定バックエンドと model 選択の決定後**に backend 確定
