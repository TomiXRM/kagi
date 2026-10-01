# ADR-0212: terminal auto-lock の解除を rename による compare-and-unlock にする（ADR-0208 決定 2 の amend）

- Status: **Accepted**
- Date: 2026-10-01
- Amends: [ADR-0208](0208-terminal-autolock-foundation.md) 決定 2「未解決のまま明示する競合」。契約 A〜D は不変。
- Related: [#836](https://github.com/TomiXRM/kagi/issues/836)、[#772](https://github.com/TomiXRM/kagi/issues/772)（umbrella）、PR #835、#523（消す前に退避する）

## 文脈

ADR-0208 の自動解除は、preflight（reason が Kagi token であることを読む）の後に libgit2 の `wt.unlock()`
（`.git/worktrees/<name>/locked` の無条件 `unlink`）を呼ぶ 2 段で、atomic ではなかった。外部の git が
その間に unlock → 別 reason で relock すると、Kagi が他人の lock を消しうる。git に compare-and-unlock は無い。

## 決定

1. **compare-and-unlock（`kagi-git` の filesystem op、既存 preflight の後）**
   1. `rename(locked → locked.kagi-<pid>-<nonce>)`。単一 rename は原子的で、以後そのファイルを持つのはこの呼び出し
      だけになる。`locked` が無ければ既に解除済みとして `Ok`（idempotent）。
   2. 移した先を読み、中身（末尾の改行を除く。git CLI は `reason\n`、libgit2 は reason のみを書く）が
      この session の token なら削除して完了。
   3. 他人のものなら `hard_link(移した先 → locked)` で戻してから移した先を消し、release は refuse（Failed）。
      **戻しに `rename` は使わない**: 1 の後 git からは unlock 済みに見えるので、外部の `git worktree lock` が
      新しい `locked` を作っている可能性があり、`rename` はそれを黙って上書きする。`hard_link` は既存の
      `locked` があれば失敗するので、その場合は移したファイルを `locked.kagi-*` として残し、場所をエラーで示す
      （契約 D: 自動回収しない）。
2. **verify**: 「この session の token を持つ lock が残っていない」こと。`is_locked()` が `Unlocked`、または
   token 以外の reason（自分の lock を移した直後に他人が立てた lock）なら成功。`is_locked()` だけだと、他人が
   正しく立てた後続の lock を「解除失敗」と誤報する。
3. **残骸（契約 D）**: `locked.kagi-*` が admin dir にあれば、
   - 自動解除の plan と preflight では blocker `WorktreeNote::LockLeftover { name, dir, files }`。
   - 手動 unlock の plan card では同じ note を warning として表示し、中身を確認して `locked` に戻すか削除するよう
     促す（EN/JA）。どの経路も残骸を消さない。
4. **test seam**: `kagi_domain::worktree_autolock::AutoUnlockRace`（有限 enum: `UnlockBeforeMove` /
   `RelockBeforeMove(reason)` / `RelockAfterMove(reason)` / `RelockAroundMove { before, after }`）を
   `#[doc(hidden)] Backend::execute_auto_unlock_worktree_racing` が受け、外部 process の lock 変更を固定の点
   （move の前・後）で起こす。時間待ちでなく決定的に race を再現する。remove / stash の fault point と同じ
   「有限 enum、任意 closure を渡さない」形で、production の経路（`execute_auto_unlock_worktree`）は `None`。
5. **対象外**: release の confirm 省略（ADR-0208 Phase 2 の別論点、別 issue）。

## 結果

- 自動解除は他人の lock を消さない。`RelockBeforeMove` で戻して refuse し他人の reason が残ること、
  `RelockAfterMove` で自分の lock だけが消え他人のものが残ること、`RelockAroundMove` で戻せないとき残骸として
  保存し次の plan が blocker になること、を kagi-git integration と Tier A `terminal_auto_lock_race` で固定する。
- 無条件 unlink に戻すとこれらのテストが落ちる（変異で確認）。
- crash で 1 と 2 の間に落ちた場合も、残骸が plan に現れ、人が確認して戻すか削除する。
