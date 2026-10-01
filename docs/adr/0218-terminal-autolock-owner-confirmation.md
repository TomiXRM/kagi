# ADR-0218: terminal auto-lock の所有権は acquire 成功後だけ記録する（#772 Phase 2a）

- Status: **Accepted**
- Date: 2026-10-01
- Amends: [ADR-0208](0208-terminal-autolock-foundation.md) 決定 1・4。解除の原子性は [ADR-0212](0212-terminal-autolock-compare-and-unlock.md) のまま。
- Related: #772、#835（Phase 1）、#836（compare-and-unlock）、#900（`RepoIdentity` は未統合）。

## 文脈

既定 OFF の設定を ON にした場合だけ、Phase 1 は acquire の **plan** 時点で `KagiTerminalSession.auto_lock` に token と対象を保存していた。ユーザーがカードを Cancel しても、同じ reason の外部 lock が後から作られると shell 終了時に解除カードを提示できた。また終了時の backend はその時点の active tab の `repo_path` を開き、別 repository の modal を上書きしえた。PID と tab 番号だけの token はアプリ再起動後に再利用されうる。

## 決定

1. **所有権の確定。** acquire card は `AutoLockOffer { owner: SessionId, generation, path, target: AutoUnlockTarget }` を保持するだけ。`KagiTerminalSession.auto_lock` は空のまま。承認時に同じ session / spawn 世代、稼働中の shell、現在の worktree identity を再観測し、既存の `execute_lock_worktree` の **成功後のみ**同じ provenance を session に保存する。Cancel、blocker、失敗、承認前の shell 終了は所有権を作らない。終了した shell の未承認カードは閉じる。
2. **token と識別子。** process ごとに OS から 128-bit の random nonce を取得し、tab incarnation と shell generation とともに token に含める。entropy が得られなければ自動 acquire 自体を見送り、予測可能な PID fallback は使わない。#900 が main に統合されるまでは `RepoIdentity` に依存せず、attach 時に凍結した `SessionId`・worktree locator path・`WorktreeId` を承認時と解除時に再照合する。backend を開いた**後にも** identity を比較し、変化した場合は書き込まず拒否する。
3. **解除は元の owner のみ。** `ShellExit::Exited` かつ保存した spawn generation が一致する場合だけ、凍結 path の backend で token / identity 照合つき `plan_auto_unlock_worktree` を作る。別タブが表示中、または別 modal が使用中なら提案を保留し、元タブへ戻ったとき・root で modal が閉じたときに再提示する。ユーザーのカードを上書きしない。承認時にも owner / generation / repository / backend identity / 保存した claim を再確認し、成功したときだけ claim を消す。明示的な Cancel または同じ worktree の手動解除成功も claim を消す。Operation Log の repository は凍結した owner path を使う。
4. **観測不能と crash。** `ShellExit::Unknown` は exit ではなく、解除提案を作らない。tab close やアプリ crash で session が無くなれば自動で残存 lock を回収せず、reason を示す既存の確認付き手動 unlock で回復する。確認済みの lock が保留中なら新 shell の起動で claim を黙って捨てない。設定を acquire 後に OFF にしても、実際に取得した lock の解除確認は提供する。

## 範囲と結果

既定 OFF の利用者には挙動変更なし。ON でも acquire と release はそれぞれ明示 confirm が必要。cwd の移動追跡、複数 terminal の集約、confirm 省略は行わない。macOS の native PTY Tier A は未承認 Cancel / shell 先行終了、別 repository の同名 worktree と競合 modal、元 owner への保留配送、手動 relock 拒否、設定 OFF 後の実際の解除と owner path の durable receipt を観測する。#836 の compare-and-unlock は維持する。
