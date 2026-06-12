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
