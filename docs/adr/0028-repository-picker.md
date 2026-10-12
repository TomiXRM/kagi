# ADR-0028: Repository Picker と Welcome 画面

- Status: Accepted / Date: 2026-06-12

## Decision

- **ディレクトリ選択は gpui ネイティブ**: `cx.prompt_for_paths(PathPromptOptions {
  files: false, directories: true, multiple: false, prompt: Some("Open Repository") })`
  (macOS の NSOpenPanel。oneshot Receiver を `cx.spawn` で await)
- 選択後 `open_repository(path)` で検証。**git repo でない / 開けない場合は tab を作らず**
  error toast(W3-NOTIFY)+ footer で理由を表示
- **Welcome 画面**: tab が 0 枚(引数なし起動 / 全 tab close)のとき、従来の usage エラー画面に
  代えて中央に「Open Repository…」ボタン + 説明文を表示。ボタン → picker
- picker の起点は3つ: Welcome ボタン / tab strip の [+] / (later: cmd-o)
- CLI 引数は従来どおり有効(初期 tab になる)。引数が repo でない場合は Welcome + error toast
- headless では picker は開けない → `KAGI_OPEN_REPO=<path>` で代替(ADR-0027)

## Consequences

- `with_error` ベースの既存エラー画面は「repo open 失敗を welcome 上の toast で表す」形に
  置き換わる(エラー文字列自体は維持し headless ログ互換を保つ)
- サブディレクトリ選択(repo 内の深い path)は git2 の discover に頼らず
  open_repository の既存挙動に従う(失敗なら理由を出す)

## Amendment — 2026-10-13: Create Worktree chooser ownership (#1383)

The Add Worktree folder button (#1043) still requests one existing parent
directory through `prompt_for_paths`; the field becomes `<parent>/<branch>`
using the default path's branch-name sanitization. Selection is input to the
existing live plan, never directory creation or a ref write.

Freeze the requesting path Input entity ID and the existing session
`Attachment` (session + visit) before opening the chooser. Apply its answer
only while both still match. The Input identity is the form lifetime; there
is no additional modal generation counter. Cancelling/replacing the card,
leaving the repository tab, and departing then returning all retire the
answer, even if the new card has the same branch name. A surviving form
continues to use its current branch name and the normal typed-input replan.

The opt-in Tier A transport queues immediate or deferred oneshot answers;
both it and the native receiver use the same asynchronous completion task.
At 940×660 and 167%, EN/JA coverage checks replacement and departed-visit
rejection, unchanged path/touched/plan and no replan for stale/cancelled
answers, one edit/replan for the requesting form, and absence of directory
or branch creation. This is deterministic state coverage, not a claim that
native NSOpenPanel permits every concurrent UI action or VoiceOver evidence.
