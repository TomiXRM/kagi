# ADR-0090: Smart Commit のメッセージ品質改善 + 生成中スピナー

- Status: Accepted(2026-06-17、ユーザー依頼「commit message 生成が微妙。OpenCommit のようにローカル LLM でも綺麗に出したい。生成中は async + ローディングアニメーション(braille の点々スピナー)」)
- Date: 2026-06-17
- 関連: ADR-0044(Smart Commit / Ollama 連携)、message_gen

## Context

Smart Commit の LLM 生成は動くが、出力が「微妙」。原因はプロンプトが一行の指示しかなく、ローカルモデルが Conventional Commits 形式や imperative mood を外しやすいこと。OpenCommit はローカルモデルでも綺麗な subject を出すが、その差は **プロンプトの作り込みと生成パラメータ**にある。

生成はすでに `background_spawn` で非同期化済み(`run_smart_generation`)。不足していたのは生成中の視覚的フィードバック。

## Decision

### プロンプト(`message_gen::build_prompt`)を OpenCommit 流に作り直す

- 役割提示(expert software engineer)+ STAGED diff を1つの subject に要約する明確な指示。
- Conventional Commits の許可 type 一覧(feat/fix/docs/style/refactor/perf/test/build/ci/chore/revert)と `<type>(<scope>): <subject>` の形を明示。Plain スタイルは type 無しの一行。
- ルール: imperative mood(added でなく add)、何が変わったか具体的に、72字未満、末尾ピリオド無し、diff に無い変更を捏造しない、言語指定、**出力は subject のみ**(引用符・コードフェンス・前置き・説明を禁止)。
- 期待出力の例を1つ示す(few-shot がローカルモデルの形式遵守を大きく改善する)。
- 言語: Ja のときも type とコード識別子は英語のまま。

出力は引き続き **subject 一行**に限定する(`clean_llm_message` は据え置き)。body を許すとローカルモデルが冗長な説明を足しやすく、品質が安定しないため。v1 は「綺麗な subject」に集中する。

### 生成パラメータ(`ollama_generate_request_body`)

- `options.temperature = 0.2`、`top_p = 0.9` で決定的・形式遵守寄りに。
- `num_predict = 128` で subject を超えて喋り続けないよう緩く上限(thinking を切れば subject は短いので十分)。

### thinking モデル対策(`think:false` + リトライ)— 今回の本丸

ローカルの reasoning モデル(例: gemma4:31b)は、reasoning に出力予算を使い切って `response` が**空**で返る(`done_reason:"length"`)。Kagi はこれを「LLM 失敗」として黙って rule-based にフォールバックしていた。これがユーザーの「微妙」の主因。

対策: `/api/generate` に **`think:false`** を付けて思考をスキップさせ、subject を直接答えさせる。実機(gemma4:31b)で空 → `feat(api): refresh the access token on a 401 response` と一発で綺麗に出ることを確認。

ただし thinking 非対応の plain instruct モデルは `think` フィールドを拒否しうるので、`ollama_generate` は **まず `think:false` で試行 → 失敗/空なら `think` 無しで1回リトライ**する(`ollama_generate_once` を think 指定付きで2通り呼ぶ)。これで thinking 系・非 thinking 系の両方で動く。

### 生成中スピナー

- Suggest ボタンは生成中、**braille の点々スピナー**(`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`)+「Generating…」を表示する。`with_animation(.. .repeat())` でフレームを回す(busy snackbar と同じアニメ基盤)。パネルは各フレームで再描画されるのでクロージャは毎回 1 子要素の div を組み直す。
- 生成は read-only かつ既に非同期なので `busy_op` ゲートは不要。完了でボタンは通常の Suggest に戻る。

## Consequences

- ローカルモデルでも Conventional Commits 形式・imperative mood の subject が安定して出る(プロンプト + 低 temperature + 例)。
- 生成中はスピナーで状態が分かる。ウィンドウは固まらない(従来どおり非同期)。
- subject 一行方針は維持。body 生成は将来課題。
- テスト: `request_body_escapes_quotes_and_newlines` は `"stream":false` を含むため options 追加後も通る。`clean_strips_fences_and_quotes`(subject 一行)も不変。
