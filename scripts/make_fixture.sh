#!/usr/bin/env bash
# テスト用 fixture repo 生成スクリプト(開発時の動作検証は必ずこの repo に対して行う)。
#
# 使い方:
#   scripts/make_fixture.sh [DEST]
# DEST 省略時は mktemp -d で /tmp 配下に生成する。最終行に repo のパスを出力する。
#
# 生成内容:
#   $DEST/remote.git  bare repo(origin として使用)
#   $DEST/repo        作業 repo:
#     - main: merge commit を含む履歴、tag v0.1.0、origin/main より 1 commit ahead
#     - feature/one: merge 済み branch(push 済み)
#     - feature/two: origin/feature/two より 1 commit behind
#     - stash 1 件、modified + untracked な working tree
set -euo pipefail

DEST="${1:-$(mktemp -d /tmp/kagi-fixture.XXXXXX)}"
case "$DEST" in
  /tmp/*|/private/tmp/*|/var/folders/*) ;; # 安全のため tempdir 配下のみ許可
  *) echo "error: DEST must be under /tmp (got: $DEST)" >&2; exit 1 ;;
esac
if [ -e "$DEST/repo" ]; then
  echo "error: $DEST/repo already exists; refusing to overwrite" >&2
  exit 1
fi
mkdir -p "$DEST"

git init -q --bare "$DEST/remote.git"
git init -q -b main "$DEST/repo"
cd "$DEST/repo"
git config user.name "Fixture"
git config user.email "fixture@example.com"
git config commit.gpgsign false
git remote add origin "$DEST/remote.git"

commit() { # commit <file> <content> <message>
  echo "$2" >> "$1"; git add -A; git commit -qm "$3"
}

commit README.md "# fixture" "initial commit"
commit a.txt "a" "add a.txt"

git checkout -qb feature/one
commit f1.txt "f1" "feature one work"
commit f1.txt "f1b" "feature one more"

git checkout -q main
commit b.txt "b" "add b.txt"
git merge -q --no-ff feature/one -m "merge feature/one"
git tag v0.1.0

git checkout -qb feature/two
commit f2.txt "f2" "feature two work"
commit f2.txt "f2b" "feature two more"

git checkout -q main
git push -q -u origin main feature/one feature/two

# origin/feature/two より 1 commit behind にする(fixture 内なので reset 可)
git checkout -q feature/two
git reset -q --hard HEAD~1

# origin/main より 1 commit ahead にする
git checkout -q main
commit c.txt "c" "add c.txt (unpushed)"

# stash 1 件
echo "dirty for stash" >> a.txt
git stash push -qm "wip on a.txt"

# working tree を汚す(modified + untracked)
echo "modified" >> b.txt
echo "untracked" > untracked.txt

echo "$DEST/repo"
