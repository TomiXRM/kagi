# ADR-0059: 3-way + Result View アーキテクチャ

- Status: Accepted(2026-06-13)

## Decision

- 4役割(Current / Incoming / Base / Result)。MVP はファイル単位 choose + Result preview、
  v0.2 で 3-pane + 編集可能 Result(uniform_list 仮想化必須 — VSCode の巨大ファイル凍結が反例)
- 粒度の段階: file → hunk → line/手編集(将来 symbol)。non-conflicting 一括適用(JetBrains 流)
- 各 hunk に blame-of-sides(両側の最終 commit sha+summary)で「意図」を見せる(kagi 独自)
- **退路**: inline marker テキストをそのまま編集するモードを常に残す(VSCode 反発の教訓)
- キーボード: accept current/incoming/next-unresolved を最初からショートカット化
- binary / rename-delete / modify-delete は専用カード UI(全 GUI が弱い差別化点)
