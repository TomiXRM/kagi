# ADR-0219: リポジトリ選択画面と clone の書き込み(#923)

- Status: **Accepted**
- Date: 2026-10-02
- Related: #923、#924(organization のリポジトリ)、ADR-0028(ディレクトリ選択と Welcome)、ADR-0188(subprocess runner)、ADR-0177(Unknown の扱い)。

## 文脈

タブ帯の `+` は OS のフォルダー選択を直接開くだけで、最近開いたリポジトリにも GitHub のリポジトリにも届かない。Kagi には clone が無く、GitHub 上のリポジトリを使い始めるには外で clone してから開く必要があった。clone は新しい書き込み(フォルダーを作る)で、しかも開く前のリポジトリが無いので、既存の書き込み経路(`finish_run` はタブの session と `Backend::open(repo_path)` で admission する)にそのままは乗らない。

## 決定

1. **画面は window 単位の modal。** `+` と New Tab(⌘T、`file.newTab`)は選択画面を開き、Esc で閉じる。「Open Repository…」(`file.openRepository`)は従来どおりフォルダー選択を直接開く。中身は最近開いたリポジトリ(`recent_repos()`)、自分の GitHub リポジトリ(`gh repo list`)、「フォルダーを開く…」と Remote Browse への導線。File History などの takeover はタブの状態にぶら下がるが、この画面はどのタブにも属さず結果が新しいタブなので、Remote Browse と同じ window 単位(`is_repo_scoped() == false`)の modal にし、タブが無い Welcome 画面でも ⌘T で同じ画面が出る。Welcome 画面自体は変えない。
2. **一覧は自分のリポジトリだけ、件数を明示。** `gh repo list --limit 1000`。`gh` は既定 30 件で黙って止まるので、上限に達したら「さらにあり」を表示する。organization のリポジトリは #924。
3. **clone は `gh repo clone <host>/<owner>/<repo> <dest>`。** 一覧を取った `gh` の認証と protocol 設定で clone するので、`git` に credential が無くても private / Enterprise のリポジトリを clone できる。fork なら `gh` が `upstream` remote を足すことを card に書く。
4. **triple は `crates/kagi-git/src/ops/clone.rs`。** `plan_clone`(source と clone 先、拒否理由)→ confirm → `preflight_clone(request, plan)`(実行する request が承認した plan と同じ source・clone 先・fork 表示であること、plan に blocker が無かったこと、同じ検査をやり直す。違えば `PlanMismatch` で Refused、`gh` は呼ばない)→ `execute_clone` → `verify_clone`(clone 先が repository として開け、`origin` が読める host なら source と一致)→ oplog。receipt は `op = "clone"`、repo は clone 先のパス。Success / Refused / Failed / Unknown / Partial(開けるが origin が別)。
5. **上書きしない、消さない。** clone 先が既にあり空のフォルダーでなければ(ファイル・symlink・中身のあるフォルダー)拒否する。clone が失敗・中断して何かが残っても Kagi は消さず、そのパスを card と receipt に出す。消すかどうかはユーザーが決める。
6. **期限は 30 分、止めるのは自分の起動した clone だけ。** 通常の 60 秒では大きなリポジトリの clone が途中で切れる。期限を過ぎたら `run_child` が、この clone のために Kagi が起動したプロセスグループ(`gh` とそれが起動した `git`)だけを止め、outcome は Unknown にする。ユーザーが起動したプロセスや他のプロセスには触れないので、「ユーザーのプロセスには触れない」方針と矛盾しない。v1 にキャンセルは無い(途中まで書かれたフォルダーの扱いと合わせて別 issue)。
7. **admission は window 単位の single-flight。** clone にはタブの session もリポジトリも無いので、同時に 1 件だけ実行する。成功したら新しいタブで開き、recent に加える。clone 先の既定は最後に clone した親フォルダー(`clone_parent_dir`)配下の `<repo>`、初回は最近開いたリポジトリの親、無ければ HOME。card で変更できる。

## 結果

PR を 3 つに分ける。PR1 = 本 ADR、triple、`gh repo list` の読み取り、integration test。PR2 = 選択画面(最近 / フォルダーを開く / Remote Browse)。PR3 = GitHub 一覧と clone の card / 実行の配線、Tier A(fake `gh` とローカル bare repository)。キャンセル、organization、clone の進捗表示(% 表示)は範囲外。
