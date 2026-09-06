//! Fixed POSIX-shell protocol programs for the remote stash transport.

pub(super) const COMMON_DIR_SCRIPT: &str = r#"set -eu
root=$1
cd -P -- "$root"
root=$(pwd -P)
test "$(git rev-parse --is-inside-work-tree)" = true
common=$(git rev-parse --path-format=absolute --git-common-dir)
cd -P -- "$common"
printf 'KAGI-COMMON-DIR\0%s\0KAGI-END\n' "$(pwd -P)""#;

pub(super) const STATE_SCRIPT: &str = r#"set -eu
root=$1
cd -P -- "$root"
head=$(git rev-parse --verify HEAD)
oids=$(git stash list --format=%H | paste -sd, -)
index_path=$(git rev-parse --git-path index)
if test -f "$index_path"; then index=$(git hash-object "$index_path"); else index=missing; fi
worktree=$(git status --porcelain=v2 -z --untracked-files=all | git hash-object --stdin)
printf 'KAGI-STATE\0%s\0%s\0%s\0%s\0KAGI-END\n' "$head" "$oids" "$index" "$worktree""#;

pub(super) const DROP_SCRIPT: &str = r#"set -eu
root=$1; expected_common=$2; operation=$3; job=$4; index=$5; selected=$6
expected_head=$7; expected_oids=$8; expected_index=$9; shift 9; expected_worktree=$1; scope_digest=$2
cd -P -- "$root"
common=$(git rev-parse --path-format=absolute --git-common-dir)
cd -P -- "$common"; common=$(pwd -P); cd -P -- "$root"
state() {
  head=$(git rev-parse --verify HEAD)
  oids=$(git stash list --format=%H | paste -sd, -)
  index_path=$(git rev-parse --git-path index)
  if test -f "$index_path"; then index_hash=$(git hash-object "$index_path"); else index_hash=missing; fi
  worktree=$(git status --porcelain=v2 -z --untracked-files=all | git hash-object --stdin)
}
state
before_head=$head; before_oids=$oids; before_index=$index_hash; before_worktree=$worktree
refuse() {
  printf 'KAGI-STASH-DROP\0%s\0preflight\0%s\0%s\0\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0preflight\0KAGI-STASH-END\n' \
    1 1 "$selected" "$before_head" "$before_oids" "$before_index" "$before_worktree" \
    "$before_head" "$before_oids" "$before_index" "$before_worktree"
  exit 0
}
if test "$common" != "$expected_common" || test "$head" != "$expected_head" ||
   test "$oids" != "$expected_oids" || test "$index_hash" != "$expected_index" ||
   test "$worktree" != "$expected_worktree"; then
  refuse
fi
runtime=${XDG_RUNTIME_DIR:-$HOME/.cache}
mkdir -p -m 700 "$runtime/kagi/remote-ops" || refuse
test ! -L "$runtime/kagi" && test ! -L "$runtime/kagi/remote-ops" || refuse
runtime=$(cd -P -- "$runtime" && pwd -P) || refuse
ops="$runtime/kagi/remote-ops"
case "$ops/" in "$root/"*|"$common/"*) refuse;; esac
owner=$(stat -f %u "$ops" 2>/dev/null || stat -c %u "$ops" 2>/dev/null) || refuse
mode=$(stat -f %Lp "$ops" 2>/dev/null || stat -c %a "$ops" 2>/dev/null) || refuse
test "$owner" = "$(id -u)" && test "$mode" = 700 || refuse
drop_out=$(git stash drop "stash@{$index}" 2>&1) && exit_code=0 || exit_code=$?
stdout_oid=${drop_out##*' ('}; stdout_oid=${stdout_oid%')'}
state
material=$(printf '%s\n' 1 complete "$exit_code" "$selected" "$stdout_oid" "$before_head" "$before_oids" "$before_index" \
  "$before_worktree" "$head" "$oids" "$index_hash" "$worktree" git)
digest=$(printf %s "$material" | git hash-object --stdin)
token="$ops/$job"; tmp="$token.tmp"
printf 'KAGI-STASH-TOKEN\0%s\0%s\0%s\0%s\0%s\0%s\0KAGI-TOKEN-END\n' \
  1 "$operation" "$job" "$scope_digest" "$digest" "$material" >"$tmp"
chmod 600 "$tmp"; mv "$tmp" "$token"
printf 'KAGI-STASH-DROP\0%s\0complete\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0git\0KAGI-STASH-END\n' \
  1 "$exit_code" "$selected" "$stdout_oid" "$before_head" "$before_oids" "$before_index" \
  "$before_worktree" "$head" "$oids" "$index_hash" "$worktree"
exit "$exit_code""#;

pub(super) const READ_TOKEN_SCRIPT: &str = r#"set -eu
job=$1
runtime=${XDG_RUNTIME_DIR:-$HOME/.cache}
token="$runtime/kagi/remote-ops/$job"
test -f "$token"; test ! -L "$token"; cat -- "$token""#;
