かなり相性の良い方向性です。設計の中心を **「Issue管理」ではなく「思いつきを実装可能な仕事へ変換する」** に置くと、GitHub本家やLinearとの差別化がはっきりします。

私ならコンセプトを **「Twitter for Engineering Intent」** くらいまで割り切ります。Issueはチケットではなく「技術的な投稿」で、そこから議論・分解・AI実装・PRへ育っていくものです。

### まず、Issue作成画面を主役にする

GitHubのように、

`Issues → New issue → repository → title → metadata → body → submit`

と進ませるのはやめた方がいいです。

起動直後から、中央にこういうComposerがあるくらいでいいです。

````text
┌──────────────────────────────────────────────────────┐
│ kagi.app                                             │
├──────────┬───────────────────────────────────────────┤
│ Inbox    │ sushi-maker-hardware ▾                    │
│ Drafts 3 │                                           │
│ Issues   │ 何を直したい？                            │
│ PRs      │ ────────────────────────────────────────  │
│          │                                           │
│          │ USB2CANが切断したあと、自動復帰しない。   │
│          │                                           │
│          │ ```                                       │
│          │ usb 2-3.1: USB disconnect                 │
│          │ ```                                       │
│          │                                           │
│          │ candumpも以降何も流れなくなる。            │
│          │                                           │
│          │ + Add context     Markdown | Preview      │
│          │                                           │
│          │          [Create Issue] [→ Implement]     │
├──────────┼───────────────────────────────────────────┤
│          │ Recent                                    │
│          │ ● USB reconnect ...                  #421 │
│          │ ● Refactor conveyor state ...        #420 │
└──────────┴───────────────────────────────────────────┘
````

**タイトルすら最初は必須にしなくていい**と思います。

Twitterで「タイトルを書いてください」と言われたら投稿頻度が激減します。本文を書いたあと、

> ✨ 「USB2CAN切断後にCAN通信が復帰しない」

とAIにタイトルを生成させればいい。

これはかなり重要です。Issueを書く際の「ちゃんとしたIssueを書かなきゃ」という心理的負担を消せます。

---

## 「Quick Capture」と「Full Editor」を分ける

Twitter的な軽さと長文Issueは、一つのUIだけでは両立しにくいです。

なのでComposerを二段階にします。

最初は、

```text
┌────────────────────────────────────────┐
│ What's happening?                      │
│                                        │
│ ConveyorControllerでraceしてそう       │
│                                        │
│ [repo ▾]                    [Post ↑]   │
└────────────────────────────────────────┘
```

くらい。

書き始めて数行を超えたり、コードを貼ったら自然に拡張される。

````text
┌─────────────────────────────────────────────┐
│ ConveyorControllerでraceしてそう            │
│                                             │
│ 再現条件                                    │
│ - ejectとreceiveが同時                      │
│                                             │
│ ```python                                   │
│ async with self.lock:                       │
│     ...                                     │
│ ```                                         │
│                                             │
│ ──────────────────────────────────────────  │
│ 📎 2 files   <> code   / commands           │
│                                             │
│ Write      Preview                          │
│                                             │
│                     [Create Issue]          │
└─────────────────────────────────────────────┘
````

そして `⌘⇧Enter` などで **Focus Editor** に入る。

これはNotion化するよりはるかに実装コストが低く、それでいて長文も十分書けます。

---

## MarkdownはWYSIWYGにしない方がいい

ここはかなり強くそう思います。

Notionのようなブロックエディタを作り始めると、

* selection
* undo/redo
* block nesting
* paste
* drag/drop
* IME
* code block
* Markdownとの相互変換
* cursor movement

あたりが全部プロダクト本体並みの仕事になります。

**GitHub Issueを書くためのアプリなのに、Notionを再実装することになる。**

それは目的から外れています。

最初は普通のMarkdownエディタで、

```text
Write | Preview
```

だけで十分です。

ただしMarkdownを書く苦痛はUI側で消す。

例えば選択した文字に対して、

```text
B  I  <>  Link  Quote  Checklist
```

という小さいフローティングメニューを出す。

あるいは、

```text
/
  Code block
  Checklist
  Image
  Quote
  Table
  Mermaid
```

というSlash commandを出す。

Linearも現在、Markdown入力に加えて `/` コマンドや選択時ツールバーを採用しています。Markdownを隠蔽しすぎず、入力補助だけリッチにする設計です。([Linear][1])

この塩梅がちょうどいいです。

---

# 一番面白いのは「Context」をIssueの第一級概念にすること

ここは普通のIssueクライアントと決定的に変えられます。

Issue Composerに

**`+ Add context`**

を置く。

押すと、

```text
Add Context

⌘ Current branch
⌘ Current diff
⌘ Selected files
⌘ Selected lines
⌘ Commit
⌘ PR
⌘ Terminal output
⌘ Screenshot
⌘ Clipboard
```

が出る。

例えばコードを選択して、

```text
右クリック
  → Create Issue from Selection
```

すると、

````markdown
`src/conveyor/controller.py:231-267`

```python
async def eject(...):
    ...
````

````

まで勝手に入る。

さらにGitクライアントなのだから、

```text
Commit
Diff
Branch
PR
Issue
````

全部を知っています。

これはWeb版GitHubには真似しにくい。

**Issueを書く人にコンテキストを手入力させない。**

ここがkagi.appの圧倒的に強い部分になり得ます。

---

# AIは「文章を書く機能」より「Issueを実行可能にする機能」にする

ありがちな、

> ✨ Rewrite with AI

だけでは弱いです。

Issue Composerの下に小さく、

```text
AI
  整理する
  再現手順を抽出
  Acceptance Criteriaを作る
  不足情報を指摘
  実装タスクに分解
```

くらいがいい。

特に面白いのは、

```text
雑なメモ

「camera落ちるとcomponent_container全部死ぬ。
realsenseだけ別processにしたほうがいいかも」
```

↓

```markdown
## Problem

RealSense nodeがクラッシュすると、
同一component_container内の他ノードも終了する。

## Current behavior

...

## Expected behavior

RealSense障害が他コンポーネントに伝播しない。

## Proposed approach

- RealSenseを別processへ分離
- restart policyを追加

## Acceptance criteria

- [ ] RealSenseを切断してもMainUnitは継続
- [ ] RealSense再接続後に復帰可能
```

みたいになる。

でも**原文は絶対消さない**方がいいです。

```text
Original | Refined
```

で保持する。

AIに「きれいなIssue」にされる過程で、重要なニュアンスを勝手に削られる事故を防げます。

---

# そして主役は「Create Issue」ではなく「Implement」

ここがこのプロダクトの核になりそうです。

投稿後、

```text
#421 USB2CAN disconnect recovery

────────────────────────────────

USB2CANが切断されたあと...

────────────────────────────────

🤖 Implement

  Codex
  Claude
  Local Agent

  Branch
  issue/421-usb-recovery

             [Start implementation]
```

とする。

つまり、

```text
Thought
   ↓
Issue
   ↓
Agent
   ↓
Branch / Worktree
   ↓
Commit
   ↓
PR
```

が一本につながる。

**IssueがAI AgentへのPrompt兼SSoTになる。**

これはかなり強いコンセプトです。

---

## Agentの会話をIssueコメントに全部流さない

これは避けた方がいいです。

例えば、

```text
Issue
 ├ Human discussion
 ├ PR
 └ Agent Run
      ├ Investigating...
      ├ Read 14 files
      ├ Hypothesis
      ├ Tests
      └ Changes
```

と分ける。

Agentが

> controller.pyを確認します
> 次にtestsを確認します
> あ、違いました
> 別の可能性があります

みたいなのをGitHubコメントとして100件残したら最悪です。

人間向け履歴とAgentの作業ログは分離した方がいい。

---

# Issue詳細も「Twitter Thread」に寄せる

例えば、

```text
Tomix
#421 USB2CAN disconnect recovery
2h

USB2CANが切断されたあと、自動復帰しない。

...

♡ 3   💬 5       Implement
─────────────────────────────

Alice
Reproduced on NUC.

USB hub抜き差しで100%起きる。

─────────────────────────────

Agent
Implementation ready

+ 143 - 38
7 tests added

PR #425 →

─────────────────────────────

Reply...
```

Issueを「フォーム」ではなく、**Thread**として扱う。

GitHub Issueとの精神的互換性も高いです。

---

# FeedもIssue一覧ではなくTimelineにする

現在のGitHub的な

```text
#123 bug ...
#124 feature ...
#125 fix ...
```

だけではなく、

```text
┌──────────────────────────────┐
│ conveyor · #421        2h    │
│                              │
│ USB2CAN disconnect recovery  │
│                              │
│ 切断後、自動復帰しない。     │
│ candumpも止まる。            │
│                              │
│ bug   CAN                    │
│                              │
│ 🤖 Implementing   PR #425    │
└──────────────────────────────┘
```

という投稿カードにする。

Twitterそのものに寄せすぎる必要はありませんが、

**「読む → 思いつく → その場でReply/Create」**

の流れはかなり参考になります。

---

## metadataは隠す

Issue作成時に、

```text
Assignee
Labels
Project
Milestone
Type
Parent issue
Priority
```

を全部見せるのは入力体験を破壊します。

Composerには、

```text
repo-name  + Add property
```

程度でいい。

`+` を押したら、

```text
Label
Assignee
Parent
Type
Project
Milestone
```

が出る。

いわゆる段階的表示です。

AIに

> `bug`, `CAN`, `hardware`

などを候補表示させてもいい。

---

# 現在のGitHubなら「Issue分解」もかなり面白い

GitHubは現在、Sub-issuesを正式に持っていて、親子関係を作れます。さらにIssue dependenciesもあり、2026年には `gh issue` からtype・parent/sub-issue・dependenciesまで操作できるようになっています。([GitHub Docs][2])

なので、

```text
✨ Break down
```

すると、

```text
#421 Camera process isolation

Sub-issues

○ #422 Separate RealSense process
○ #423 Add process watchdog
○ #424 Add reconnect integration test

Dependencies

#423 blocked by #422
#424 blocked by #422
```

をAIが提案し、

```text
[Create 3 sub-issues]
```

でGitHub上にも実体を作れる。

これは**AI Coding時代のIssue Client**として非常に自然です。

ちなみにGitHub自身も古いTasklist Blocksを退役させ、Sub-issuesを後継として案内しています。([GitHub Docs][3])

---

# 私なら最初のMVPはこれだけにします

機能を盛り始めると確実にIssue版Notionになって死ぬので、最初はかなり絞ります。

1. **常時表示Issue Composer**
2. **Markdown textarea**
3. **Markdown Preview**
4. **コード貼り付け**
5. **画像Drag & Drop**
6. **repo選択**
7. **Draft自動保存**
8. **Issue作成**
9. **Issue → Agent**
10. **Issue Thread表示**

そして次に、

```text
Context attachment
    ↓
AI refinement
    ↓
Sub-issue decomposition
    ↓
Agent / Worktree / PR
```

を足す。

---

## GPUIについて

GPUIでもこの方向なら十分狙えると思います。

ただし、

> Notion相当のBlock Editor

を最初から目指すのはやめた方がいいです。

**Markdown source editor + renderer** にすれば、かなり現実的になります。

難所はMarkdownの描画よりむしろ、

* 日本語IME
* selection
* undo/redo
* multiline
* syntax highlight
* 巨大テキスト
* clipboard
* drag/drop

あたりの「ちゃんとしたテキストエディタ」です。

なのでEditor自体を独立コンポーネントとしてかなり真面目に作っておく価値があります。

---

### このプロダクトのキャッチコピーまで落とすなら

私は、

> **Write it. Ship it.**

あるいはコンセプトとして、

> **Issue is where implementation starts.**

くらいが合うと思います。

今までのGitHub Clientは、

> Repositoryを操作する

ソフトでした。

kagi.appはむしろ、

> **開発者の思考をGitHub上の成果物に変換する**

ソフトにした方が面白いです。

その意味では **Issues → Agent → Worktree → PR** を一筆書きで繋げるところが、Issue画面単体のデザイン以上に重要になると思います。

[1]: https://linear.app/docs/editor?utm_source=chatgpt.com "Editor – Linear Docs"
[2]: https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/adding-sub-issues?utm_source=chatgpt.com "Adding sub-issues - GitHub Docs"
[3]: https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/about-tasklists?utm_source=chatgpt.com "About tasklists - GitHub Docs"
