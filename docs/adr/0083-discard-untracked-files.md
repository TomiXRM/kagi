# ADR-0083: Discard untracked files (delete with ODB backup)

- Status: Accepted(2026-06-15、ユーザー依頼「新たに追加したファイルを unstaged から discard all したら消えて欲しい」)
- Date: 2026-06-15
- Supersedes: ADR-0046 の「untracked file は対象外(MVP)」判断(§Decision / §Consequences の該当箇所)

## Context

ADR-0046 は untracked(新規追加)ファイルの discard を **対象外** とした。理由は
「untracked の discard = ファイル削除 = `git clean` 相当であり、`git clean` 実装禁止の
codebase 規約に抵触する」ため、ボタンを disabled にしていた。

しかし実利用では「雑に追加したファイルをまとめて捨てたい」= **untracked も含めて消したい**
という要望が強い(ユーザー報告)。Discard all が tracked の変更だけ戻して untracked を
残すのは直感に反する。

ポイントは「`git clean` 禁止」の本当の理由 = **復元不能な破壊を作らないこと** であって、
ファイル削除そのものではない。ADR-0046 が tracked discard に課した
**backup-then-discard**(捨てる前に内容を ODB に blob 化して oplog に記録)を untracked にも
適用すれば、削除しても `git cat-file -p <sha>` で復元可能 = kagi の安全性 thesis を保てる。
これは「内容を一切残さない `git clean -f`」とは本質的に異なる。

## Decision

### 意味論(ADR-0046 を更新)

- **Discard(untracked file)** = 対象ファイルを **working tree から削除**する。
  - 削除前に必ず内容を ODB に blob 化してバックアップする(下記)。
  - index には元々載っていないので index は不変(tracked と同じく staged には触れない)。
- **Discard(tracked file)** は従来どおり index 内容で WT を上書き(`git checkout -- <path>`)。
- **Discard all changes** = unstaged セクションの対象を全部 discard。tracked は復元、untracked は削除。
  - **conflicted のみ skip**(従来どおり。conflict 解決フローで扱う)。untracked はもう skip しない。

### 安全機構: backup-then-discard(untracked にも必須)

`execute_discard` は対象を tracked / untracked に分類し:

1. **backup**(全対象共通): 各ファイルの現在の WT 内容を `repo.blob()` で ODB に書き込み、
   `path → blob SHA` を集める。1 件でも失敗したら discard 全体を中止(WT 無変更)。
2. **apply**:
   - tracked → `checkout_index`(path 指定・force)で index 内容に戻す。
   - untracked → `std::fs::remove_file` でディスクから削除(blob 化済みなので復元可能)。
3. **verify**: tracked は status の unstaged から、untracked は untracked セット/ディスクから消えたこと。
4. **oplog**: `discard` op として path→blob SHA を記録(復元手段は tracked と同一)。

`git clean` は使わない(空ディレクトリの掃除等はしない。削除するのは対象ファイルのみ)。

### UI

- per-file: unstaged 行の**右クリック → 「Discard changes…」**を untracked 行でも出す
  (ADR-0046 の「untracked にはメニューを出さない」を撤回)。
- Discard all: untracked を一覧の **対象** に含める(skipped ではなく)。plan が
  「N untracked file(s) will be deleted from disk(backed up to the oplog first;
  recover with `git cat-file -p <blob-sha>`)」という **warning** を出すので、確認 modal は
  削除であることを明示する(blocker ではない = 実行可能)。
- danger 確認 modal / async 実行 / `KAGI_AUTO_CONFIRM` は ADR-0046 のまま。

## Consequences

- 「破壊的操作はしない」規約の例外管理に、untracked 削除を **backup-then-discard 隔離フロー限定で許可**
  として追記(ADR-0046 の tracked discard と同型の例外)。`git clean` 実装禁止の規約は維持
  (kagi は `git clean` を呼ばず、削除前に必ず ODB バックアップを取る点で別物)。
- ODB に untracked ファイルぶんの backup blob が増えるが、捨てた内容の対価として妥当
  (tracked discard と同じ扱い)。将来「oplog から restore」UI が untracked にも効く。
- ADR-0046 の「untracked 削除は引き続き不可能」は本 ADR で撤回。
