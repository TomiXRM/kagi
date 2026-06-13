# ADR-0061: Conflict LLM 補助の差し込み点(later)

- Status: Accepted(2026-06-13、実装は later — 設計予約のみ)

## Decision

- 差し込み点を2つに固定: (a) hunk の両側意図の要約 (b) Result 草稿への解決案挿入(適用はユーザー)
- ADR-0044 のモデルを踏襲: localhost Ollama のみ / 明示 opt-in / 送るのは対象 hunk ±文脈と
  両側 commit message のみ / 初回同意ダイアログ / KAGI_OFFLINE で停止
- **自動解決はしない**(提案まで)。ユーザー所感「ローカル LLM は core feature になりそう」を
  踏まえ、ResolutionBuffer の API は提案挿入を一級でサポートする形に設計しておく
