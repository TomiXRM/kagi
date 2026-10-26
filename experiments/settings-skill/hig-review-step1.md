# HIG design review r1 — Settings window → `settings_kit` (sidebar + grouped form)

- 対象: `target/ui-lab/design/figure.png`(左 = 現行 Tier B、右 = 提案。同じ論理スケール)と提案の要約
- 方法: `.claude/skills/hig-review/SKILL.md` Step 1(実装前のデザインレビュー)
- 出典: `~/.claude/skills/apple-design` はこのマシンに入っていない。代わりに Apple 公式の `.md` を使った:
  `target/ui-lab/hig/*.md`(依頼で渡されたもの)と、今回 `developer.apple.com/design/human-interface-guidelines/<page>.md` から取得した
  `target/ui-lab/hig-extra/{steppers,accessibility,buttons}.md`。引用はすべてこれらのファイルからの逐語引用。
  iOS/iPadOS 節にある文(例: toggles の "Use the switch toggle style only in a list row"、sidebars の "Consider using a tab bar first")は
  macOS の根拠にしていない。

---

## 1. 指摘

### [P2] 設定ウィンドウのナビゲーション: HIG は toolbar のペインを想定。sidebar はその代わりとして明示されていない

`settings.md › Platform considerations › macOS`:
> "Typically, a custom settings window contains a toolbar that includes buttons for switching between views — called *panes* — that each contain a group of related settings."
>
> "**In your settings window, use a noncustomizable toolbar that remains visible and always indicates the active toolbar button.** A settings window’s toolbar identifies the areas people can customize and helps people navigate among those areas. People rely on a stable settings interface to help them find what they need."

この文が求めているのは「安定していて、カスタマイズできず、常に表示され、どのペインがアクティブか常に分かる」ペイン切り替えである。前置きが "Typically" なので、P1(本文との矛盾)ではなく P2 とした。
なお現行も titlebar の toolbar ではなく、コンテンツ内の横タブ列なので、現行も文字どおりには従っていない。

sidebar 側の根拠(`sidebars.md` 冒頭)は
> "A sidebar appears on the leading side of a view and lets people navigate between areas of your app or top-level collections of content"

で、7 カテゴリを切り替える用途には合っている。ただし同じページの
> "A sidebar requires a large amount of vertical and horizontal space."(`sidebars.md › Discussion`)

のとおり、幅の代償(715 → 900 pt)がある。

**修正(どちらかを owner が選ぶ):**

- **A(HIG どおり)**: 現行の横タブを titlebar のペイン用 toolbar(アイコン+ラベル、カスタマイズ不可)に置き換える。detail 側のグループ化フォームは提案どおり使う。
- **B(提案を維持)**: sidebar にする場合は、settings.md の要件を sidebar で全部満たす。
  - sidebar は隠せない(sidebar トグルなし)。
  - 並べ替えやカスタマイズはできない。
  - 選択行は、ウィンドウが非アクティブなときも常に見える。
  - 前回のペインを復元する。

  この逸脱は `docs/decisions.md` に 1 行で記録し、根拠には上の settings.md の文と、macOS 自身の System Settings が採っている形であることを挙げる。

  `sidebars.md` には
  > "When possible, let people customize the contents of a sidebar."

  とあるが、設定の安定性を優先して従わない。この点も同じ行に書く。

### [P2] sidebar のアイコンタイルが 7 色の固定色で、色に意味がない

`sidebars.md › Best practices`:
> "**Make sure any sidebar icon colors you choose serve a clear purpose.** By default, sidebar icons use your app’s accent color. In macOS, people can change the system accent color, which applies to all apps. When they do this, they expect all sidebar icons to appear in that color, so make sure your sidebar icons display the color people choose. However, if you use them sparingly, fixed colors can help clarify the meaning of an icon or draw attention to it."

`color.md › Platform considerations › macOS › App accent colors`:
> "Because a fixed-color sidebar icon uses a specific color to provide meaning, the system doesn’t override its color when people change the value of accent color settings."

提案は全行に別々の固定色(灰・緑・青・橙・紫・ピンク・赤)のタイルを付けている。これは "sparingly" でも "meaning" でもなく飾りである。しかも Kagi には Accent 設定(System | Theme)があるのに、sidebar だけがそれに従わない。

**修正**: タイルの塗りをやめ、単色の symbol をアクセント色(Kagi の Accent 設定の結果)で描く。System Settings 風のタイルを残すなら、全行同じ色にしてアクセント色に従わせる。固定色は意味を持つ 1 行にだけ使う。例: 問題を示す状態がある場合。

### [P2] セグメントの幅が不揃い

`segmented-controls.md › Best practices`:
> "**In general, keep segment size consistent.** When all segments have equal width, a segmented control feels balanced."

`segmented-controls.md › Content`:
> "**As much as possible, use content with a similar size in each segment.**"

図では System と Theme、Icon and Text と Icon Only の幅が違う。JA では「アイコンとテキスト」と「アイコンのみ」の差がさらに大きくなる。

**修正**: 各セグメントの幅を、EN/JA で最長のラベルに合わせて揃える。JA で行に収まらない場合は、その設定だけ radio 2 個(現行)に戻す。

### [P2] 小さすぎるクリック領域

`accessibility.md`:
> "**Offer sufficiently sized controls.** Controls that are too small are hard for many people to interact with and select."

同じ表の macOS 行は `|macOS |28x28 pt |20x20 pt |`(既定 / 最小)。

図の次の要素は 20×20 pt を下回って見える。

- Theme のポップアップの上下シェブロンの箱(約 10 pt)
- Theme folder 行の `›` の箱
- ズーム stepper の −/+

**修正**:

- ポップアップは、値の文字列とシェブロンを合わせた全体を 1 つのボタン(高さ ≥ 20 pt)にする。
- stepper の各セグメントは ≥ 20×20 pt にする。
- Tier B では実寸で測る。

### [P2] 「Graph」が sidebar のカテゴリと General 内のセクションの両方にある

`layout.md › Visual hierarchy`:
> "**Group related items to clearly express related information or functions.**"

sidebar に Graph ペインがあるのに、Compact graph と Avatar commit nodes が General › Graph セクションに置かれている。同じ名前が 2 か所にあると、利用者はどちらを開くべきか分からない。

**修正**: Graph の設定は Graph ペインに集める。General に残す理由があるなら、セクション名を変える(例: Appearance に統合)。

### [P2] ペインを開くボタンの省略記号と、外部へ出る行のシェブロン

`buttons.md › Platform considerations › macOS › Push buttons`:
> "**Append a trailing ellipsis to the title when a push button opens another window, view, or app.**"

「Theme folder — Open ›」は Finder(別の app)を開くボタンである。「Theme file guide ›」はガイドを開く外部リンク(`settings_theme_folder.rs` の `guide_link`)である。行末のシェブロンは「このウィンドウ内で下の階層へ進む」と読めるので、行き先を誤認させる。

**修正**:

- フォルダを開くボタンは「Open in Finder…」(JA「Finder で開く…」)のプッシュボタンにする。
- ガイドは外部リンクだと分かる表示(リンク文字、または外向き矢印)にする。
- シェブロン行は、実際にウィンドウ内で詳細ページへ進む行だけに使う。

### [house] 現行の機能・情報が図から消えている

- **Reload Themes** ボタン(`view.reloadThemes`)が無い。Custom themes セクションの trailing accessory か行内ボタンとして残す。
- **Compact layout threshold** の行(70rem の stepper と換算表示)が General に無い。移動先のペインを決める。
- Accent の副題「Mac accent: Default (Blue)」が無い。System 側を選んだときの実効色が分からなくなる。行の subtitle として残す。
- 「Add JSON files to the folder…」の説明文が無い。

**修正**: 現行の全設定キー → 新しいペイン / セクション / 行の対応表を PR に添付する。漏れが 0 であることを示す。

### [house] 「Theme」セグメントのラベルが短縮で曖昧

現行の「Theme accent」が「Theme」になり、すぐ上の行の「Theme」(テーマ選択)と同じ語になる。`segmented-controls.md › Content` は
> "**Use nouns or noun phrases for segment labels.** Write text that describes each segment"

を求めている。

**修正**: 「System | Theme」を維持するなら、行の subtitle に実効色を出す(上の house 指摘)。または「System Accent | Theme Accent」と書く。JA のラベルは `kagi-ja-wording` 方針で簡潔にする。

### [P2→確認事項] 設定ウィンドウの最小化・拡大ボタン

`settings.md › Platform considerations › macOS`:
> "**Dim a settings window’s minimize and maximize buttons.**"

図では両方とも 3 つの信号ボタンが同じ灰色で、無効化されているか判別できない。

**修正**: 新しいウィンドウでも minimize/zoom を無効化する。Tier B ではアクティブ状態で撮影して確認する。無効化されていなければ merge review で P2 とする。

### [P3] switch のサイズ

`toggles.md › Platform considerations › macOS › Switches`:
> "**Within a grouped form, consider using a mini switch to control the setting in a single row.** The height of a mini switch is similar to the height of buttons and other controls, resulting in rows that have a consistent height."

提案の文章は mini switch だが、図の switch は約 38×22 pt で regular サイズに見える。Compact graph の行の高さが、他の行と揃っていない。

**修正**: 実装では mini サイズにする。高さはセグメントやポップアップと同じ程度にし、行の最小高 40 pt を保つ。図も直す。

### [P3] stepper の形

`steppers.md › Discussion`:
> "A stepper sits next to a field that displays its current value, because the stepper itself doesn’t display a value."

「− 100% +」は値を 2 つのボタンで挟む形で、HIG の stepper(値の横に 2 セグメント)とは違う。

**修正**: 値と、その横の −/+ の 2 セグメント(連結したもの)にする。現行の部品で十分なら変えなくてよい。

`steppers.md › macOS`:
> "consider supporting Shift-click to change the value quickly"

は範囲が 0.7–2.0 と狭いので不要。

### [P3] 戻る/進む(‹ ›)と検索

ウィンドウ内で詳細ページへ進む行が実際にない限り、‹ › は押しても何も起きないボタンになる。上の指摘でシェブロン行が無くなれば、‹ › も削除する。

検索フィールドは、7 ペインと少数の設定には過剰である。`settings.md › Best practices`:
> "**Minimize the number of settings you offer.** … too many settings can make the experience feel less approachable, while also making it hard to find a particular setting."

設定数が少ないうちは検索を省いてよい。入れる場合は ⌘F で検索にフォーカスでき、該当ペインへ移動できるようにする。

### [P3] ウィンドウ幅

`settings.md › macOS`:
> "because a settings window accommodates the size of the current pane, people don’t need to expand the window to see more."

900 pt 固定にするなら、高さはペインに合わせるか、スクロールしても行の位置が飛ばないようにする。

---

## 2. 判定

VERDICT: PROCEED

(P1 は無い。P2 は実装時に上の修正で解消し、merge review(Step 2)で再確認する。sidebar か toolbar かは owner の選択事項。)

---

## 3. 個別の判断

### sidebar か toolbar ペインか(macOS の設定ウィンドウ)

HIG の既定は toolbar のペインである(`settings.md › macOS` の "Typically … a toolbar" と "use a noncustomizable toolbar that remains visible and always indicates the active toolbar button")。sidebar は直接には許可されていない。

ただしこの文の本質は「安定・常時表示・アクティブ表示」で、"Typically" なので P1 ではなく P2 とした。現行の横タブも文字どおりには従っていない。

推奨は次のどちらか。

- A: titlebar の toolbar ペインにする(HIG に最も近い)。
- B: 隠せない・カスタマイズ不可・選択が常に見える sidebar にし、`docs/decisions.md` に逸脱として記録する。

ペインのタイトルとウィンドウタイトルの連動(`settings.md › macOS`: "Update the window’s title to reflect the currently visible pane.")と、前回のペインの復元("Restore the most recently viewed pane.")は、どちらの案でも必須で、提案は満たしている。

### 2 択でセグメントか radio か(グループ化フォーム内)

HIG はどちらも認めている。

- `toggles.md › macOS › Radio buttons`: "**Prefer a set of radio buttons to present mutually exclusive options.**"、"Typically displayed in groups of two to five"
- `segmented-controls.md › Best practices`: "**Use a segmented control to provide closely related choices that affect an object, state, or view.**"、"**Consider a segmented control when it’s important to … clearly show their selection state.**"

禁止されているのは表示の切り替えに使うことだけである(`segmented-controls.md › macOS`: "Use a tab view in the main window area — instead of a segmented control — for view switching")。Accent と Toolbar は状態の選択なので該当しない。

判断: テキストだけのセグメントは可。ただし次の条件を付ける。

- セグメント幅を揃える(P2)。
- JA で行に収まること。
- ラベルが名詞で、意味が一意であること。

収まらない設定だけ radio に戻す。

### グループ化された行の switch サイズ

mini switch にする(`toggles.md › macOS › Switches` "Within a grouped form, consider using a mini switch…")。提案の文章は正しいが、図は regular サイズに見える(P3)。

現行もすでに switch なので、"In general, don’t replace a checkbox with a switch." には抵触しない。

### アイコンタイルの色(sidebars.md の色の指針)

7 色の固定色は "Make sure any sidebar icon colors you choose serve a clear purpose" と "if you use them sparingly, fixed colors…" に反する(P2)。アクセント色に従う単色にし、固定色は意味がある場合に限る。

### ポップアップボタンの見た目

値+上下シェブロンは、macOS のグループ化フォームのポップアップとして妥当である。行のタイトル「Theme」が選択肢を予告している(`pop-up-buttons.md › Best practices`: "**Give people a way to predict a pop-up button’s options without opening it.** For example, you can use an introductory label")。選択肢は排他的なフラットリストなので、ポップアップを使うのが正しい(`pop-up-buttons.md`: "**Use a pop-up button to present a flat list of mutually exclusive options or states.**")。

問題はクリック領域だけである。値とシェブロンを合わせた全体を 1 つの押せる領域(≥ 20×20 pt、`accessibility.md`)にする(P2)。シェブロンだけが押せる見た目にはしない。
