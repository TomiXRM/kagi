# ADR-0037: コミットアバターの GitHub 取得

- Status: Accepted / Date: 2026-06-13

## Context

アバターは現在 FNV-1a 色 + イニシャルの自前円。ユーザー要望: upstream サービス
(当面 GitHub のみ)から実アイコンを取得したい。

## Decision

- **有効条件**: いずれかの remote URL が `github.com` を指す repo のみ有効。
  それ以外の repo は現行のイニシャル円のまま
- **email → avatar の解決(優先順)**:
  1. **noreply パース**: `<id>+<user>@users.noreply.github.com` / `<user>@users.noreply.github.com`
     → `https://avatars.githubusercontent.com/<user>?s=64`(API 不要・即時)
  2. **Commits API バッチ**: `GET https://api.github.com/repos/{owner}/{repo}/commits?per_page=100`
     (未認証、数ページまで)から `commit.author.email → author.avatar_url` のマップを構築。
     1 リクエストで最大 100 commit 分解決でき、rate limit(60/h)に優しい
  3. 解決不能な email → 現行イニシャル円に**フォールバック**(エラー表示はしない)
  - email 単体を外部に送る user-search API は**使わない**(privacy + rate limit)
- **取得と描画**:
  - HTTP は **gpui 同梱の http client が 0.2.2 で使えるか先に調査**し、不可なら
    `ureq`(blocking・小依存)を追加してよい(**本 ADR が Cargo.toml 変更の根拠**)。
    取得は background(`cx.background_spawn`)で行い UI を塞がない
  - 画像は `~/.kagi/avatars/<sha1(url)>` にディスクキャッシュ(再起動後はネット不要)+
    メモリキャッシュ(email → Arc<gpui::Image>)。デコードは gpui の `img()` /
    `ImageSource::Image`(png/jpeg 内蔵)に任せる
  - 表示箇所: commit row のアバター円・Inspector メタ行。**取得完了までイニシャル円を表示し、
    届き次第差し替え**(レイアウトは円のまま、画像は rounded_full クリップ)
- **オフライン/制御**: `KAGI_OFFLINE=1` でネット取得を完全停止(headless テストは常に offline で
  決定的に)。取得失敗は静かにフォールバックし、リトライは session 中 1 回まで
- 認証トークンは v0 では使わない(60req/h で十分: repo ごと数リクエスト + キャッシュ)

## Consequences

- 初の「ネットワークへ出る」機能。送信するのは GitHub への repo 座標と avatar URL のみ
  (ユーザーの email を検索クエリにしない)
- ureq 追加時は依存純度がわずかに下がる(gpui http が使えればゼロ)
- GitHub 以外(GitLab 等)は将来の resolver 追加で拡張(interface はそれを見据えて trait 化しない
  — YAGNI、関数分岐で十分)
