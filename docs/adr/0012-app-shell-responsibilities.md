# ADR-0012: App Shell Layout and Panel Responsibilities

- Status: Accepted / Date: 2026-06-12

## Decision

6スロット構成と責務を確定する(T-BP-001 の slot 化を正式仕様化):
- **Header Toolbar**: Git 操作の入口 + 「いま pull/push すべきか」の判断材料(branch/upstream/↑↓)。左=状態、中央=操作、右=メタ操作(Refresh/Search/Settings)
- **Left Sidebar = Repository Navigator**: repo 内の「場所」への移動(branch/remote/tag/stash)。操作は context menu 経由で plan へ
- **Main Center**: commit graph(または全幅 diff)。常に「歴史」を見せる場所
- **Right Panel = Commit Inspector**(または Commit Panel): 選択対象の「詳細と文脈的操作」。情報が先、操作が後
- **Bottom Panel**: 補助コンソール(Terminal / Operation Log)。主画面を侵食しない(default ≤ 画面高20%)
- **Status Bar**: repo 状態の常時サマリ + Bottom Panel の入口

## Consequences
- 新 UI 要素は必ず上記いずれかの責務に帰属させる(複数スロットに同種機能を散らさない。例外: ↑↓は判断材料として Header と Status Bar に重複可)
