# ADR-0123: Avatar resolution via public lookups (Gravatar + GitHub user search)

- Status: Accepted
- Date: 2026-07-19
- Supersedes: ADR-0037 の解決順序(noreply parse + repo Commits API のみ)。
  ADR-0037 のキャッシュ/オフライン/フォールバック設計はそのまま生きる。

## Context

kagi は ADR-0037 で「author のメールアドレスを検索クエリとして外部に送らない」
方針を採り、解決手段を (1) GitHub noreply parse、(2) その repo の Commits API
バッチ(owner/repo しか送らない)に限定していた。その結果、rgitui など
公開ルックアップを行うクライアントでは表示できる author が kagi では
イニシャルサークルのままになるケースが多数あった:

- Gravatar にだけ登録している author
- GitHub プロフィールにメールを公開している author(user search でヒット)
- remote が GitHub でない / private / 直近 300 コミットに現れない author

ユーザー判断(2026-07-19): 「公開されているメールアドレスからアカウントの
アイコンを取りに行く行為にプライバシー上の問題は特にない」— 公開情報への
ルックアップをデフォルトで行う方針に変更する。

また調査で ADR-0037 実装の取りこぼしバグが 2 件見つかった:

1. `parse_commits_api` のペアリングずれ — 実レスポンスは
   `commit.author.email` → `commit.committer.email` → `author.avatar_url` →
   `committer.avatar_url` の順で並ぶため、「email から次の email までの窓で
   最初の avatar_url」を拾う方式では author.email が常に不発
   (author == committer のときだけ committer 側の窓が偶然正解を拾う)。
   squash-merge / Web UI コミット(committer=noreply@github.com)しかない
   author はマップから漏れる。
2. `ensure_avatars` の 1 リポジトリ 1 回きりガード — 初回ロード時の rows に
   いた author しか解決されず、load more / reload / 外部コミットで後から
   現れた author は対象外。

## Decision

1. **解決順序を拡張する**(email ごと、上から順に、最初のヒットで確定):
   1. GitHub noreply parse(無料・確実)
   2. repo の Commits API バッチ(GitHub remote の repo のみ、従来どおり)
   3. **Gravatar** — `sha256(trim+lowercase(email))` で
      `https://www.gravatar.com/avatar/<hash>?s=64&d=404`(404 = 未登録)
   4. **GitHub user search** — `GET /search/users?q=<email>+in:email&per_page=1`
      でプロフィール公開メールから login を引く。未認証 search の
      レートリミット(10 req/min)を尊重して 1 パスあたり上限
      [`MAX_SEARCH_LOOKUPS`] 件に制限する。
   - 3/4 は `KAGI_OFFLINE=1` で完全に無効(既存契約どおり)。設定は設けない
     (デフォルト ON。要望が出たら settings.json キーを足す)。
   - 氏名での search(`in:fullname`)は**採用しない** — 同名他人の誤ヒットで
     「別人のアイコンが出る」リスクがあり、safety-first に反する。
2. **`parse_commits_api` を serde_json ベースに書き換え**、
   `commit.author.email ↔ author.avatar_url` / `commit.committer.email ↔
   committer.avatar_url` を正しくペアにする(serde_json は settings.rs で
   既に依存済み)。
3. **`ensure_avatars` を増分解決にする** — `AvatarStore` に試行済み email 集合
   と `view_epoch` スナップショットを持ち、reload / load more / タブ切替で
   rows に新しく現れた email だけを追加解決する(render 毎フレームの
   コストは epoch 比較 1 回)。
4. 依存は既存ロックの範囲で賄う: `sha2`(Gravatar ハッシュ)と
   `percent-encoding`(search クエリの email エンコード)を bin の直接依存に
   昇格するのみ。新規 crate なし。

## Consequences

- rgitui で表示できて kagi で出なかった author(Gravatar 登録者・メール公開
  GitHub ユーザー・非 GitHub remote の repo)が表示されるようになる。
- author のメール(および sha256 ハッシュ)が gravatar.com / api.github.com に
  送信される。公開コミットメタデータ由来の情報であり、ユーザー判断により
  許容する。`KAGI_OFFLINE=1` が完全な opt-out として機能する。
- 未認証 search のレートリミットにより、大量の未解決 email がある repo では
  1 パスで解決しきれないことがある(上限件数で打ち切り、次の増分パスで継続)。
- `[kagi] avatar: resolved=N pending=M offline=B` の行形式は不変。増分解決に
  伴い、この行は「新規 email があったパスごと」に出るようになる。
