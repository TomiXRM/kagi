# ADR-0013: Header Toolbar Git Operation UX

- Status: Accepted / Date: 2026-06-12(ADR-0009 を UX 面で補完)

## Decision
- 配置: 左=repo名・current branch・upstream・↑↓ / 中央=Pull(↓N)・Push(↑N)・Branch・Stash・Pop・Undo・Terminal / 右=Refresh・Search(later)・Settings(later)
- **カウント紐付け**: Pull ボタンに ↓behind、Push ボタンに ↑ahead を直接表示。**behind=0 なら Pull は disabled**(fetch は Refresh 側の責務に移す: Refresh = fetch なし再読込のまま、明示 fetch は later に分離検討)。ahead=0 なら Push disabled(既存)
- **Undo の明示**: 有効時はラベル/ツールチップに対象 commit(`Undo "<summary>"`)。対象なしは disabled
- 全ボタン plan 経由・force/reset-hard/clean 禁止(ADR-0009 の再確認)

## Consequences
- 「Pull が押せる=取り込むものがある」が UI 不変条件になる(ローカル知識基準。鮮度は ADR-0010)
