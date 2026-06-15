# ADR-0086: 統一された sync-icon スナックバー(進行中 / no-op 表示)

- Status: Accepted(2026-06-16、ユーザー依頼「時間のかかる関数は async にしつつ、スナックバーで sync icon がぐるぐるしてて欲しい」「すでに origin と差分がない時の Push/Pull は popup ではなくスナックバーで」「async のスナックバーは全部同じ UI に統一」「no-op の時も sync icon を大きく」)
- Date: 2026-06-16
- Builds on: ADR-0079(merge を含む長時間 op の plan→confirm→execute)、トーストシステム(`Toast` / `ToastKind` / `push_toast`)

## Context

長時間かかる git 操作(merge / pull / push / stash / checkout / commit / …)は
すでに `busy_op` を立てて `cx.background_spawn` で UI スレッド外に逃がしていた
(ウィンドウが固まらない)。しかし進行中表示が **2 系統に分裂** していた:

1. ヘッダ左の小さな refresh-cw アイコン(`busy_op` で回転)。
2. 各 op が `push_toast(ToastKind::Info, Msg::Started…)` で出す
   「X: started」トースト(**小さい ⟳ テキストグリフ**)。

その結果:

- merge は **plan フェーズに Started トーストが無い** ため、大きいアイコンの
  スナックバーだけが出る = 見た目がきれい。
- それ以外の op は「started」トーストが出るため、merge とだけ見た目が違う。
- さらに「すでに最新」な Push/Pull は **わざわざ plan モーダル(ポップアップ)** を
  開いて blocker/空プレビューを見せていた。差分が無いだけなのに確認 modal は過剰。

ユーザーの要望は一貫して **「進行中・完了(no-op)を問わず、sync icon のスナックバーで
統一」**。

## Decision

### 1. busy スナックバー(進行中)

- `render_toasts` は `busy_op.is_some()` の間、トーストスタック最上段に
  **busy スナックバー** を描く。`busy_op` で自動駆動するので、新しい async op を
  足しても追加実装は不要。
- アイコンは `icons/refresh-cw.svg` を `Animation::repeat()` + `Transformation::rotate`
  で連続回転。サイズは **32px(ヘッダスピナーの約 2×)**、ラベルとの間隔は
  `gap_3`(12px = 通常トーストの 1.5×)。ラベルは `busy_label(op)`(例 "Merging…"
  "Pulling…")。
- 各 op の **`Msg::Started…` トーストは全廃**。進行中インジケータは busy スナックバー
  1 本に統一した(`Started*` の Msg variant と i18n も削除)。

### 2. no-op の Push/Pull はスナックバー(モーダルにしない)

- `open_push_modal`: blocker が「nothing to push」のみ(= upstream 設定済みかつ
  ahead==0)なら、plan モーダルを開かず `ToastKind::Sync` のスナックバーを出す。
- `open_pull_modal`: blocker/warning 無し かつ behind==0(タイトルの
  "up to date (local knowledge…)" で判定。background auto-fetch が behind を
  更新し続ける)なら、空の確認モーダルではなくスナックバーを出す。
- **実 op は従来どおり**:実際に push/pull するもの(ahead/behind>0、blocker/warning
  あり)は確認モーダルを開く。no-op を勝手に実行することはしない。

### 3. sync-icon の統一(`ToastKind::Sync`)

- 新 variant `ToastKind::Sync` を追加。busy スナックバーと **同一の 32px 回転 sync
  icon**(共通ヘルパ `big_sync_icon()`)で描画する。
- no-op の "already up to date — nothing to pull/push" はこの `Sync` を使う。
  → 進行中(busy)と完了(no-op)で sync icon の見た目が完全に一致する。
- 他の `ToastKind::{Info,Success,Error}` は従来の小さいテキストグリフのまま
  (✓ / ✕ / ⟳)。

## Consequences

- 進行中表示は **busy スナックバー 1 本** に集約。op を追加しても `busy_op` を
  立てるだけで一貫した表示が得られる(`Started*` トーストの追記は不要・禁止)。
- sync を伴うスナックバー(進行中・no-op)は **同じ大きい回転アイコン** で統一。
  ユーザーが op ごとの見た目差に気を取られない。
- ポップアップが減る:差分が無い Push/Pull は非ブロッキングのスナックバーで済む。
- `Sync` トーストは回転アニメーションを持つ。no-op は「動いていない」状態だが、
  ユーザー要望(sync icon を進行中と揃える)を優先し回転させている。短時間で
  auto-dismiss(4s)するため違和感は小さい。将来「静止アイコンにしたい」要望が
  来たら `Sync` の描画だけ差し替えればよい(他は不変)。
- i18n: `AlreadyUpToDatePull` / `AlreadyUpToDatePush`(en/ja)を追加、`Started*` を削除。
