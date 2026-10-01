# ADR-0214: Operation Log パネルを一級ビューにする — 全体設計と slice 1(読むだけ)

- Status: **Accepted**(slice 1)。slice 2 以降は本 ADR の設計を前提に個別に Accepted にする
- Date: 2026-10-01
- Related: [#334](https://github.com/TomiXRM/kagi/issues/334)、#333 / ADR-0149(actor・worktree・id/parent)、ADR-0081 / ADR-0084(undo)、ADR-0111(`OpLogPanel`)、#468 / #548(行の展開・コピー)

## 文脈

Kagi の操作はすべて oplog を通るが、UI では bottom panel の Operation Log tab が時刻・op 名・outcome を並べるだけで、誰が(人 / MCP / CLI)、どの worktree で行い、その操作で ref がどう動いたかが見えない。#334 はこの一覧を「行を選んで時点に戻す / 1 操作だけ取り消す / 実行前にグラフで見る」入口にすることを求めている。

`OpLogEntry` の `before` / `after` は表示用の文字列(`"branch: main"` など)で、object id を持たない。

## 決定

### 1. 全体設計(#334 §4)

- **パネル**: 既存の `OpLogPanel`(`src/ui/oplog_panel.rs` + `oplog_render.rs`、bottom panel の Operation Log tab)を強化する。置換や別画面は作らない。
  - 1 行 = 1 操作。
  - 行に表示するもの: actor バッジ(Human / MCP / CLI)、worktree バッジ、outcome の色分け。
  - 行を選ぶと詳細(before / after / error / 対応する reflog 行)を出す。
- **3 つの操作**: いずれも `plan → confirm → preflight → execute → verify → oplog` を通し、「戻す操作」自体も oplog に載る(戻したことも戻せる)。

  | 操作 | 意味 | 実装の骨子 |
  |---|---|---|
  | restore to point | その時点の ref 状態へ戻す | checkpoint + イベント列から目標 ref 状態を再構成し、ref 更新の plan を作る |
  | revert one op | 1 操作だけ打ち消す | その操作の before / after 差分だけを逆適用する。後続操作が同じ ref を動かしていれば blocker |
  | preview | 実行前に結果のグラフを描く | in-memory でグラフを再計算し、現在のグラフと並べる |

- **confirm の内容**: 実行後グラフのプレビュー、逆操作の一覧、「できないこと」(untracked の削除は戻らない等)の明示。
- **未決**(slice 2 以降で決める): #334 §5 の論点。working tree も戻すか、Refused / Failed / Partial の時点の扱い、`--keep` 相当、プレビューを並べるか差分ハイライトか。

### 2. slice 1 の範囲: 読むだけ

- **行**
  - actor バッジ: `OpLogEntry.actor`。人は控えめ、MCP / CLI は色付き。
  - worktree バッジ: `worktree`(無ければ `repo`)のディレクトリ名。full path は tooltip。
  - outcome の色分け: 既存のまま。
- **選択した行の詳細**: 既存の before / after / error に、その操作の **reflog 行** を加える。
  - 読み取りは `Backend::reflog_lines_between`(`crates/kagi-git/src/backend/reflog.rs`)。HEAD(worktree ごと)と `refs/heads/*` の reflog を、時刻範囲で拾う。
  - 選択時に background で 1 回読む。
  - 新しい選択が来たら古い結果は捨てる(generation)。結果は entry の同一性に結びつけ、行の位置には結びつけない。
- **書かないもの**: repository への write も oplog への追記もしない。restore / revert / グラフプレビューは slice 2 以降。

### 3. reflog 行の帰属は時刻窓で決める。曖昧は明示し、前提に使わない

entry は outcome が確定した時点(ref が動いた後)に時刻を刻む。そこで、同じ worktree の直前の entry の秒を下限とする `(prev, this]` を、その操作の窓とする(`kagi_domain::oplog_reflog::ReflogWindow`)。

- **同じ worktree の判定**: 隣接する entry は同じ worktree のものに限る。別 worktree の行は飛ばす。
  - `Backend::run` は workdir を末尾 `/` 付きで記録し、他の経路は付けないので、末尾の区切りを除いて比較する。
- **同じ秒の境界**: 同じ worktree の別 entry が同じ秒に記録されていれば、その秒の reflog 行は **`Ambiguous`** として表示する(「別の操作と同じ秒 — どちらの操作のものか判別できません」)。どちらの操作にも帰属させない。
- **下限が無い場合**: 読み込まれた範囲に直前の entry が無ければ、下限を 60 秒前とし、見出しでそう明示する。
- **既知の限界**: 窓は時刻だけで決まるので、Kagi の外(terminal の git など)で同じ時間帯に動いた ref も窓に入る。見出しは「この worktree の前の操作以降の reflog」であり、因果関係を主張しない。
- **slice 2 以降への拘束**: restore / revert / preview は、この時刻窓の帰属を前提にしてはならない。特に `Ambiguous` の行を、復元先や逆適用対象の根拠にしてはならない。
- **将来の正攻法**: slice 2 で oplog entry に「動かした ref と old / new OID」(`RefMove { refname, old, new }` の列)を持たせる。時刻窓は表示の補助に下げ、時点復元と取り消しは OID で厳密に行う。この変更は oplog スキーマの追加になる(#333 の互換規則に従い additive)。

## 結果

- Operation Log の各行で、誰が・どの worktree で行った操作かが一目で分かる。選択すると、その時間帯に動いた ref が reflog の形で読める。
- テスト
  - domain unit: 窓、同じ秒の曖昧、下限なし。
  - panel unit: 隣接は同じ worktree のみ、末尾 `/` の表記揺れ。
  - kagi-git integration: 2 操作の窓に、それぞれの操作の reflog 行だけが入る。読み取りは何も書かない。
  - Tier A `oplog_actor_reflog`
    - 3 actor の記録が 3 行になり、それぞれのバッジが描画される。
    - 実クリックで選んだ checkout が自分の HEAD 行だけを示す。
    - 共有された秒の行は `Ambiguous` として描画される。
    - repo の指紋と oplog の件数が不変。
- 変異確認: 曖昧判定を外す、または隣接を worktree で絞らないと、それぞれ上記のテストが落ちる。
