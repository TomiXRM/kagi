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

pub(super) const RUNTIME_ROOT_SCRIPT: &str = r#"set -eu
runtime=${XDG_RUNTIME_DIR:-$HOME/.cache}
test -d "$runtime"; test ! -L "$runtime"
cd -P -- "$runtime"
printf 'KAGI-RUNTIME\0%s\0KAGI-END\n' "$(pwd -P)""#;

pub(super) const DROP_SCRIPT: &str = r#"set -eu
root=$1; expected_common=$2; operation=$3; job=$4; index=$5; selected=$6
expected_head=$7; expected_oids=$8; expected_index=$9; shift 9
expected_worktree=$1; scope_digest=$2; expected_runtime=$3
cd -P -- "$root"
root=$(pwd -P)
common=$(git rev-parse --path-format=absolute --git-common-dir)
cd -P -- "$common"; common=$(pwd -P); cd -P -- "$root"
stat_dir() {
  stat_path=$1; stat_exact_mode=$2
  test -d "$stat_path" && test ! -L "$stat_path" || return 1
  if stat_value=$(stat -f '%u %Lp' "$stat_path" 2>/dev/null); then
    :
  elif stat_value=$(stat -c '%u %a' "$stat_path" 2>/dev/null); then
    :
  else
    return 1
  fi
  set -- $stat_value
  test "$#" -eq 2 || return 1
  stat_owner=$1; stat_mode=$2
  test "$stat_owner" = "$(id -u)" || return 1
  case "$stat_mode" in ''|*[!0-7]*) return 1;; esac
  if test -n "$stat_exact_mode"; then
    test "$stat_mode" = "$stat_exact_mode" || return 1
  else
    test $((0$stat_mode & 0022)) -eq 0 || return 1
  fi
}
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
  printf 'KAGI-STASH-DROP\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0KAGI-STASH-END\n' \
    1 preflight 1 "$selected" '' "$before_head" "$before_oids" "$before_index" "$before_worktree" \
    "$before_head" "$before_oids" "$before_index" "$before_worktree" \
    preflight
  exit 0
}
if test "$common" != "$expected_common" || test "$head" != "$expected_head" ||
   test "$oids" != "$expected_oids" || test "$index_hash" != "$expected_index" ||
   test "$worktree" != "$expected_worktree"; then
  refuse
fi
runtime=${XDG_RUNTIME_DIR:-$HOME/.cache}
stat_dir "$runtime" '' || refuse
runtime=$(cd -P -- "$runtime" && pwd -P) || refuse
test "$runtime" = "$expected_runtime" || refuse
case "$runtime/" in "$root/"*|"$common/"*) refuse;; esac
ops="$runtime/kagi/remote-ops"
case "$ops/" in "$root/"*|"$common/"*) refuse;; esac
umask 077
if test ! -e "$runtime/kagi"; then mkdir "$runtime/kagi" || refuse; fi
stat_dir "$runtime/kagi" 700 || refuse
if test ! -e "$runtime/kagi/remote-ops"; then mkdir "$runtime/kagi/remote-ops" || refuse; fi
stat_dir "$ops" 700 || refuse
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
printf 'KAGI-STASH-DROP\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0KAGI-STASH-END\n' \
  1 complete "$exit_code" "$selected" "$stdout_oid" "$before_head" "$before_oids" \
  "$before_index" "$before_worktree" "$head" "$oids" "$index_hash" "$worktree" git
exit "$exit_code""#;

pub(super) const READ_TOKEN_SCRIPT: &str = r#"set -eu
root=$1; expected_common=$2; expected_runtime=$3; expected_token=$4
cd -P -- "$root"; root=$(pwd -P)
common=$(git rev-parse --path-format=absolute --git-common-dir)
cd -P -- "$common"; common=$(pwd -P); cd -P -- "$root"
test "$common" = "$expected_common"
stat_dir() {
  stat_path=$1; stat_exact_mode=$2
  test -d "$stat_path" && test ! -L "$stat_path" || return 1
  if stat_value=$(stat -f '%u %Lp' "$stat_path" 2>/dev/null); then
    :
  elif stat_value=$(stat -c '%u %a' "$stat_path" 2>/dev/null); then
    :
  else
    return 1
  fi
  set -- $stat_value
  test "$#" -eq 2 || return 1
  stat_owner=$1; stat_mode=$2
  test "$stat_owner" = "$(id -u)" || return 1
  case "$stat_mode" in ''|*[!0-7]*) return 1;; esac
  if test -n "$stat_exact_mode"; then
    test "$stat_mode" = "$stat_exact_mode"
  else
    test $((0$stat_mode & 0022)) -eq 0
  fi
}
runtime=${XDG_RUNTIME_DIR:-$HOME/.cache}
stat_dir "$runtime" ''
runtime=$(cd -P -- "$runtime" && pwd -P)
test "$runtime" = "$expected_runtime"
case "$runtime/" in "$root/"*|"$common/"*) exit 1;; esac
ops="$runtime/kagi/remote-ops"
case "$ops/" in "$root/"*|"$common/"*) exit 1;; esac
stat_dir "$runtime/kagi" 700
stat_dir "$ops" 700
token=$expected_token
test "$token" = "$ops/${token##*/}"
test -f "$token"; test ! -L "$token"
if stat_value=$(stat -f '%u %Lp' "$token" 2>/dev/null); then
  :
elif stat_value=$(stat -c '%u %a' "$token" 2>/dev/null); then
  :
else
  exit 1
fi
set -- $stat_value
test "$#" -eq 2; test "$1" = "$(id -u)"; test "$2" = 600
cat -- "$token""#;
