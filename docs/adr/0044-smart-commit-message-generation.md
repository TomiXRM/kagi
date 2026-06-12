# ADR-0044: Smart Commit Message Generation

- Status: **Accepted**(2026-06-13、ユーザー決定: 案B寄り — 検出は行うが LLM 生成は既定無効・明示 opt-in)
- Date: 2026-06-13(改訂)

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

### 既定方針(ユーザー決定済み: 案B寄り)

**判断**(原文準拠):
- **Ollama の自動検出は行う**(到達確認のみ。検出結果は「Local LLM available」と表示)
- ただし **LLM 生成は既定では無効**。Smart Commit Message の**既定は rule-based 生成**
- LLM 生成は**ユーザーが明示的に有効化した場合のみ**(設定 or Generate ボタンからの有効化導線)
- **初回有効化時に「staged diff がローカル LLM に渡される」ことを明示**する同意ダイアログを出す
- 生成対象は **staged diff のみ**。**unstaged diff は絶対に含めない**
- **remote LLM / 外部 API は MVP では使わない**。対象は **localhost Ollama のみ**
- Ollama が見つからない場合は **rule-based のみで動作**する

**理由**: localhost であっても staged diff をモデルに読ませる機能であり、社内コード・secret・未公開仕様・
顧客情報が含まれうるため、既定 ON にはしない。検出自体は UX 改善になるため行う。

### モデル選択(ユーザー決定済み)

- 設定でモデル指定があればそれを使う
- 設定がなければ **LLM 機能は未設定状態**(rule-based のみ)
- installed model が **1つだけでも初回はユーザー確認を挟む**
- **複数モデルがある場合は必ずユーザーに選ばせる**(`/api/tags` で列挙)
- 選択後は設定(settings.json)に保存する

### UI 方針(ユーザー決定済み)

- **Rule-based suggestion: 常に利用可能**
- **Local LLM suggestion: Ollama detected + user enabled の場合のみ利用可能**
- **「Generate with Local LLM」ボタンを押した時だけ** staged diff を渡す(自動送信しない)
- **初回だけ確認ダイアログ**を出す(文言は以下を必ず含む):
  - "Only staged diff will be sent"
  - "Unstaged changes will not be included"
  - "The request stays on localhost Ollama"
  - "Secrets may still exist in staged diff; review before generating"

### 実装優先度(ユーザー決定済み)

| フェーズ | 内容 |
|----------|------|
| **MVP** | rule-based 生成 / Ollama 自動検出 / LLM disabled by default / staged diff only / 初回同意 UI / model selection UI / selected model persistence |
| **v0.2** | Conventional Commits mode / language selection / scope suggestion / body・test・risk セクション生成 |
| **v0.3+** | custom prompt template / repository-specific style memory / **remote LLM は検討のみ・別 ADR 必須** |

(注: 上記により本文前段の「Conventional Commits 既定」は v0.2 扱いに更新。MVP の rule-based は
プレーン形式の叩き台生成でよい)

## Consequences

- staged diff がプロセス外に出るのは「Ollama detected + user enabled + Generate 押下」が揃った時のみで、
  宛先は loopback に限定される。外部 API は別 ADR なしに実装しない
- trait なし・enum dispatch でバックエンド追加は分岐追加で済む
- ureq 再利用で依存純度を保つ
- T-COMMIT-015/016 は **unblocked**(本決定が backend 仕様)
