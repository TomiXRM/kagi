# pstack から蒸留する失敗・回復の扱い

2026-10-30 調査。[Cursor公式のpstack原本][upstream]を固定commit
`4dd1d05818d19792e7cd6e818f1667f95f7e1dbe`で読んだ採否資料。
著作権表示は Copyright (c) 2026 Lauren Tan、[MIT License][license]。
考え方の要約と短引用に留め、コード・テンプレート・Skill本文は移植していない。
backnotpropのmirrorやreshifのportable portは根拠に混ぜていない。

運用規則の単一ownerは[workflow.md「失敗・回復記録」][failure-owner]。
この文書は根拠と適応理由を説明するもので、別の必須手順を追加しない。
既存の正との比較、確認済み失敗の再現禁止、独立レビュー、安全gate、実GUI検証を維持する。

## 対照に使ったKagiの証拠

Settings検証の[公開結果][published]に対応する保存済み資料を読んだ。
以下のローカル資料は同じホストの当該ディレクトリにアクセスできる場合だけ参照できる。
公開結果のリンクだけでは、すべての生ログを外部読者が閲覧できるとは限らない。
locatorの起点は `/tmp/kagi-settings-visible-help-20261030/`。

| 証拠 | 取得できた事実と限界 |
|---|---|
| `browser-investigation.md` | 再クリックの座標がtrigger外だったことを確認。最初のmissing-IDと遅延hover差分の原因は未確定。 |
| `native-display-investigation.md`、`gates/native-settings-full.json` | native gateはexit 101。Sleep/DarkWakeとの時間的対応は強い環境診断だが、`None`を返した内部経路は未特定。 |
| `hover-diagnostic/{pixel-difference,state-comparison}.json` | 後続の2組は右端で1904/1604画素が変化し、記録した観測キーの変化はなし。最初の失敗と同一原因とは証明しない。 |
| `completion.json`、`verified-surface.json`、`gates/prepublish.json` | 計測区間、要求/保存済み倍率、後続gate、レビュー範囲を記録。以前の不合格を取り消す証拠ではない。 |

## 採用・適応する8点

### 1. 証拠が示す最小の修復へ戻る（適応）

- 原本。[fix-root-causes L13–21][root]の “Reproduce first.” と、[bug-fix L5–10][bug]の実行証拠に限定した修復。
- Kagi。ブラウザ再クリックではtriggerのy=592に対してクリック予定点が552.5だった。製品のdismiss不具合とは言えず、実際の露出領域を選ぶ検査座標の修復が支持された。
- 本PR。既存証拠から製品・検査条件・環境を分ける扱いを[失敗・回復記録][failure-owner]へ集約する。原本の再現要求は一律採用せず、確認済み失敗の再現だけを目的とする再実行を増やさない。修復後の検証は別扱い。

### 2. 同じ前提の修正を重ねる前に前提を調べる（適応）

- 原本。[attack-the-premise L9–19][premise]の “Write the premise down.”。[runtime-forensics L3–8][forensics]も実行時証拠とsourceの対応を要求する。
- Kagi。`window.display(cx).unwrap()`失敗にはSleep/DarkWakeの記録があり、Settingsでは`KAGI_GUI_E2E`がactivationを先に抑止していた。`KAGI_NO_ACTIVATE`を外す案はその経路を変えない。別の再失敗は時刻・生ログ未取得の報告であり、同じ原因とは断定できない。
- 本PR。期待、観測、推定、未解明と、次に識別できる証拠を[単一owner][failure-owner]へ残す。根拠のないfixtureの代替displayやassertion緩和を回復と呼ばず、新しい診断実装も追加しない。

### 3. 代理指標と実際の成果物を区別する（採用）

- 原本。[prove-it-works L9–20][prove]の “Read the actual value”。
- Kagi。canvas製品でDOMがtextboxだけのsnapshotでも、trigger/menuの実測geometryは取得できていた。逆にprobe登録は実際の文字のinkを証明しない。`verified-surface.json`も要求倍率と保存済み倍率を分けている。
- 本PR。toolの終了コードやテスト全成功だけで正との一致を認定せず、何を直接観測したかを[失敗・回復記録][failure-owner]に結び付ける。未取得を不存在・成功に置き換えない。

### 4. 画像差分は場所を特定してから説明する（適応）

- 原本。[visual-parity L3–8][visual]はbaselineを正とし、非zero差分を調べる。
- Kagi。最初の遅延hover失敗は比較画像が未取得だった。後続の2組では右端だけの変化と観測キー不変が得られた。scrollbarの時間依存描画は説明候補だが、初回失敗の原因確定にはならない。
- 本PR。画像差分、状態差分、取得条件を分け、説明できない差分をtooltipや製品不具合と決め付けない扱いを[単一owner][failure-owner]へ寄せる。文言削除、baseline変更、無説明のcropや許容で合格にしない。画素完全一致はその契約の検証に限り、全UIの共通合否には広げない。

### 5. 数字は範囲と限界を伴わせる（採用・適応）

- 原本。[explain-the-number L9–16][number]の “Keep the evidence with the number.”。[eval L7–23][eval]は自己申告だけの採点を避け、条件と観測を揃える。
- Kagi。`completion.json`の8966秒は02:54:24Z〜05:23:50Zの実時間で、初読以前は不明。PR1818の30h/53ケースはオーナー報告であり、本調査の測定値ではない。担当・条件・測定範囲が同じ比較試行は取得していない。
- 本PR。[実行サマリーと計測](workflow.md#実行サマリーと計測)を正とし、速度改善や因果を主張しない。失敗件数・Skill使用率を成績にせず、未取得の実稼働時間・料金を補完しない。

### 6. 意思決定の証拠を既存履歴から辿れるようにする（適応）

- 原本。[show-me-your-work L9–22, L38–63][trail]は “Keep one canonical log.”、記録は “not every action”。[build-the-lever L14–19][lever]は再実行可能な道具を重視する。
- Kagi。gateのargv・開始/終了・exit・logPathと、公開結果に対応する既存JSONを照合できた。記録を増やしても、失われた画像や時刻は回復できない。
- 本PR。定型TSVを追加せず、既存run/task/call ID、時刻、生証拠へのlocatorを[単一owner][failure-owner]のepisode要約から参照する。機微な引数や私的会話の公開は避ける。道具の新規ファイルを要求する原本の一律条件は採らず、loggerや計測器の実装済みとも書かない。

### 7. 学びの修正先をSkillに固定しない（採用・適応）

- 原本。[correct L15–30][correct]の “Write docs or agent rules last”、[encode-lessons-in-structure L3–11][structure]。[reflect L41–49][reflect]も構造で強制できるAccepted項目をBacklogへ移す。
- Kagi。過去にはBSD grepの制限でgateが何も検出せず成功した事故があり、現在の[AGENTS.md](../../AGENTS.md#verifying-changes)はuvのruleと自己検査を正にしている。注意文の追加だけで検出力は証明できない。
- 本PR。原因別の改善先をtool/API・使用法・環境・検証・設計/型/テスト・Skillから選ぶ考え方を[単一owner][failure-owner]へ統合する。新しい構造の実装は別課題とし、この文書でgate追加済みとは扱わない。

### 8. 独立レビューと、その確認範囲を保つ（採用・適応）

- 原本。[show-me-your-work L65–74][trail-review]の “Self-review is not a substitute.”。[poteto-mode L100–114][mode-review]は新しいsubagentと証拠/確度の明示を重視する。
- Kagi。`completion.json`は独立レビューが初回native4条件＋hover4画像、残りnative4条件は親確認、exact-head再レビューではないと明記している。指摘0件は未読条件までの保証にはならない。
- 本PR。[クロスレビュー](workflow.md#クロスレビュー)を正として、作成者と会話を共有する自己点検を独立レビューに数えず、対象head・証拠・未確認範囲を区別する。新たな常時panelは追加しない。

## 採らない形と、原文との差

以下はKagiで採らない案であり、原本がすべてを指示しているという批判ではない。

| 案 | 原文の実際とKagiでの判断 |
|---|---|
| すべてをSkill化 | `reflect`はSkill編集へ導くが構造対応を分離し、`correct`は文書を最後にする。第7項の原因別振り分けを採る。 |
| 定型TSV・全呼出しの手入力 | `show-me-your-work`にはTSV形式の指定があるが、全操作記録ではない。第6項の既存履歴と必要なepisodeだけへ適応する。 |
| 常時多モデルpanel | [reflect L23–39][reflect-panel]には3レビュアーとsynthesizer、`poteto-mode`には条件別の複数モデル導線がある。全タスクで常時panelを行う命令とは読まない。Kagiの既存レビュー適用範囲を維持する。 |
| 未測定host能力の断定・同一sessionの独立偽装 | 本調査の原文はこれらを推奨していない。Cursor前提のtool/model名はT3での実行可能性の証拠ではない。Kagiでは要求値/取得値/不明とレビュー独立性を区別する。 |
| 自動mergeの導入 | [poteto-mode L140–145][mode-shipping]にはshipping/autopilot-fullとmergeしないautopilot-stackがあり、[L33–34][mode-merge]には独立検証条件もある。本PRはその自律merge経路を導入せず、[既存のマージ条件](workflow.md#pr-での作業-pushレビューマージ)を保つ。 |
| pstack全量install | [README L15–32][install]にinstall手順はある。本PRは原本の読解と適応理由だけを扱い、install・hook・サービス追加はしない。 |

この調査では保存済み証拠の読解だけを行い、製品の失敗再現・修復検証・速度比較は行っていない。
本PRの変更は既存規則の整理と根拠の追加であり、新logger・自動収集・新gateの導入ではない。

[upstream]: https://github.com/cursor/plugins/tree/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack
[license]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/LICENSE#L1-L21
[failure-owner]: workflow.md
[published]: https://github.com/TomiXRM/kagi/pull/1826#issuecomment-6515670357
[root]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/principle-fix-root-causes/SKILL.md#L13-L21
[bug]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/poteto-mode/playbooks/bug-fix.md#L5-L10
[premise]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/principle-attack-the-premise/SKILL.md#L9-L19
[forensics]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/poteto-mode/playbooks/runtime-forensics.md#L3-L8
[prove]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/principle-prove-it-works/SKILL.md#L9-L20
[visual]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/poteto-mode/playbooks/visual-parity.md#L3-L8
[number]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/principle-explain-the-number/SKILL.md#L9-L16
[eval]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/poteto-mode/playbooks/eval.md#L7-L23
[trail]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/show-me-your-work/SKILL.md#L9-L63
[lever]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/principle-build-the-lever/SKILL.md#L14-L19
[correct]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/correct/SKILL.md#L15-L30
[structure]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/principle-encode-lessons-in-structure/SKILL.md#L3-L11
[reflect]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/reflect/SKILL.md#L41-L49
[reflect-panel]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/reflect/SKILL.md#L23-L39
[trail-review]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/show-me-your-work/SKILL.md#L65-L74
[mode-review]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/poteto-mode/SKILL.md#L100-L114
[mode-shipping]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/poteto-mode/SKILL.md#L140-L145
[mode-merge]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/skills/poteto-mode/SKILL.md#L33-L34
[install]: https://github.com/cursor/plugins/blob/4dd1d05818d19792e7cd6e818f1667f95f7e1dbe/pstack/README.md#L15-L32
