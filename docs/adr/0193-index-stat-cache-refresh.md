# ADR-0193: index stat cache の refresh は write 契約の対象外

- Status: Accepted
- Date: 2026-09-10
- Related: [#655](https://github.com/TomiXRM/kagi/issues/655), [#657](https://github.com/TomiXRM/kagi/pull/657), [#627](https://github.com/TomiXRM/kagi/issues/627), 不変条件 4

## 文脈

libgit2 は index の stat data と一致しない file の内容を毎回ハッシュし直す。`git status`
と違い、refresh した stat data を **書き戻さない**。したがってこのコストは何度 scan しても
永久に払い続ける。内容を変えずに file を触る操作 — build、同じ bytes を書き直す
formatter、copy からの復元 — がこの状態を作る。

50,000 file の repo で実測した:

| 状態 | libgit2 status |
| --- | --- |
| warm index | 135 ms |
| 全 file を `touch` した直後 | 3,305 ms |
| その次の scan | 3,318 ms（自己修復しない） |
| 端末で `git status` を 1 回打った後 | 143 ms |

24 倍を永久に払い、端末で `git status` を打つと直る。GUI が重く CLI が速いという体感は
これで説明できる。#627 で libgit2 が CLI の約 3 倍遅く見えていたのも、CLI だけが index を
黙って修復していたためだった。

修復手段は `GIT_STATUS_OPT_UPDATE_INDEX` で、これは `.git/index` を書く。不変条件 4 は
「Every write operation follows `plan → confirm → preflight → execute → verify → oplog`」
であり、字義どおり読めば index への書き込みもこの契約に入る。status は read path なので
confirm modal も oplog も出せない。**この衝突を解消しないまま実装してはいけない。**

## 決定

**不変条件 4 の「write operation」を、repository の観測可能な状態を変える操作と定義する。**
すなわち ref、object、index の *staged content*（どの path が、どの blob OID・mode で
staged されているか）、working tree、config のいずれかを変える操作。これらは従来どおり
`plan → confirm → preflight → execute → verify → oplog` を通る。

**index entry の stat cache の refresh はこの定義に含まれない。** `GIT_STATUS_OPT_UPDATE_INDEX`
は、blob OID が変わらない既存 entry の `stat` field だけを書き直す。path の増減、OID の
変更、mode の変更、ref・object への書き込みは起こり得ない。`git status` 自身が行う修復と
同一であり、ユーザーが観測できる repository の状態は前後で同じである。

### 許容範囲 — 呼び出し側で opt-in する

例外が及ぶのは **working tree を描画する UI refresh 1 経路だけ**である。

`working_tree_status` は `plan_*` / `preflight_*` / snapshot から 100 を超える経路で
到達し、その中には **confirm 前に走る `plan_create_branch` が含まれる**。plan が index を
書けば、書き込みがどれだけ狭くても不変条件 4 の違反である。したがって repair を
`working_tree_status` の性質にしてはならない。

- `working_tree_status` — **純粋な read のまま。** 既定であり、plan/preflight/snapshot は
  すべてこれを使う。
- `working_tree_status_repairing_stat_cache` — opt-in。UI refresh だけが呼ぶ。

この分離が無い実装（既定に `update_index` を付けるもの）はこの ADR に適合しない。

### 自分自身との lock 競合を防ぐ

watcher は同一 revision の status 読み取りを single-flight 化していない（連続保存の実測で
約 1.64 refresh/s）。したがって scan は重なり得る。両方が `index.lock` を取りに行くと
一方が敗れて fallback し、full scan をやり直す — kagi が自分自身と競合する。scan が
秒単位かかる repo ではこれが storm になる。

process 単位の repair slot を 1 つ置き、**同時に repair を試みる scan は 1 つだけ**とする。
取れなかった scan は repair を諦めて read-only scan を行う（もともと行う予定だったもの）。

なお上の例外が及ぶのは stat cache の refresh だけである。
以下は例外の対象外で、従来どおり write 契約を通る:

- index への path 追加・削除（staging / unstaging）
- entry の blob OID・mode の変更
- ref・object・config への書き込み
- working tree の変更

### 証拠を要求する

主張だけでは例外を認めない。`GIT_STATUS_OPT_UPDATE_INDEX` を使う実装は、**refresh の前後で
index の全 entry の (path, OID, mode) が一致することを示す test を持たなければならない。**
この test が無い、あるいは落ちる場合、その実装は不変条件 4 の write であり例外に入らない。

### oplog が不要な理由

oplog はユーザーが「何が起きたか」を後から追い、必要なら戻すための記録である。stat cache の
refresh は戻す対象を持たない。戻しても意味が無く（次の scan が同じ refresh をやり直す）、
戻せる状態の差も無い（OID も path も mode も変わっていない）。記録すべき事象が無いので
oplog に載せない。

### lock 競合時の fallback

書き戻しは `index.lock` を取るため、並行 git process が lock を保持する場合（`GIT_ELOCKED`）
と `.git` が read-only の場合に **失敗する**（どちらも実測で再現した）。status は read で
あり、**修復脚が走れないことを理由に読み取りが失敗してはならない。** したがって失敗した
場合は flag なしで 1 回だけやり直す。本物の status 失敗は 2 回目の試行から surface する。

この fallback は必須であり、省略した実装はこの ADR に適合しない。

## 帰結

- `working_tree_status` は index を warm に保ち、touch されただけの repo で 24 倍の
  再ハッシュを繰り返さなくなる。
- #627 の backend 比較は index の状態を揃えてから行う必要がある。stale index での測定は
  backend の性質ではなく CLI の自己修復の有無を測っている。
- 背景 thread の watcher が status を呼ぶ経路で `.git/index` への書き込みが発生する。
  lock 競合は fallback が読み取りを守るが、頻度と体感は実機で確認する。
- 不変条件 4 の文言に、この定義への参照を追加する。
- watcher の single-flight 化そのものは本 ADR の範囲外で、[#655](https://github.com/TomiXRM/kagi/issues/655) に残る。
  repair slot は repair の競合だけを防ぎ、status 読み取りの重複は防がない。
