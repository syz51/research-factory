#!/bin/sh
# Claude Code PreToolUse hook: refuses a tool call that would delete or edit a
# stored check run (`.research/lab/checks/*.jsonl`). Every `lab check` run is
# evidence; the skill says never to delete or edit stored findings. Off until
# you add it to a settings file:
#
#   "PreToolUse": [{ "matcher": "Bash|Edit|Write|MultiEdit|NotebookEdit",
#     "hooks": [{ "type": "command",
#       "command": "sh .claude/skills/research-factory/scripts/guard-check-runs.sh" }] }]
#
# It refuses:
# - Edit, Write, MultiEdit and NotebookEdit on a path under .research/lab/checks/.
# - A Bash command part (split at `;`, `&&`, `||`, `|` and newlines) that runs
#   rm, rmdir, unlink, shred, truncate, mv, `sed -i`, `perl -i`, `find
#   -delete`, `find -exec rm`, or git rm, mv, clean, checkout or restore
#   (not `restore --staged` alone), and names a path under
#   .research/lab/checks (or `checks/` from .research/lab). After `xargs`, a
#   path named anywhere in the command counts.
# - A redirect (`>` or `>>`) into .research/lab/checks.
# - rm, rmdir or `git rm` of a directory that holds stored runs: the checks
#   directory, .research/lab, .research, or a project root that has them.
# - `git clean` without -n or --dry-run while the repository has untracked
#   runs (with -x or -X, ignored runs count too).
# It reads commands as text: a command that builds the path at run time, or
# deletes a directory further above the project, passes. It is a guard
# against mistakes, not a security boundary. Anything else, any parse error,
# or no `jq` on PATH: it allows the call. It always exits 0.
set -u

command -v jq >/dev/null 2>&1 || exit 0
input=$(cat) || exit 0
get() { printf '%s' "$input" | jq -r "$1" 2>/dev/null; }

deny() {
    msg="Refused: this would delete or edit a stored check run ($1). Stored check runs in .research/lab/checks/ are never deleted or edited (research-factory skill, \"Before reporting done\", step 1). A rerun supersedes an earlier run without removing it: commit every run of the session, superseded ones included. Only a person may remove one, outside the agent."
    jq -cn --arg m "$msg" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $m}}'
    exit 0
}

tool=$(get '.tool_name // ""')
cwd=$(get '.cwd // ""')
[ -n "$cwd" ] || cwd=$(pwd)

case "$tool" in
    Edit | Write | MultiEdit | NotebookEdit)
        path=$(get '.tool_input.file_path // .tool_input.notebook_path // ""')
        case "$path" in
            */.research/lab/checks/* | .research/lab/checks/*) deny "$tool $path" ;;
        esac
        exit 0
        ;;
    Bash) ;;
    *) exit 0 ;;
esac

cmd=$(get '.tool_input.command // ""')
[ -n "$cmd" ] || exit 0

# A leading `cd <dir> &&` moves the working directory.
first=$(printf '%s' "$cmd" | sed -nE 's/^[[:space:]]*cd[[:space:]]+([^;&|[:space:]]+)[[:space:]]*&&.*/\1/p')
first=${first#\'}; first=${first%\'}; first=${first#\"}; first=${first%\"}
case "$first" in
    "") ;;
    /*) cwd=$first ;;
    *) cwd=$cwd/$first ;;
esac

# Whether a text names a stored run, read from directory $2.
names_runs() {
    printf '%s' "$1" | grep -q '\.research/lab/checks' && return 0
    case "$2" in
        */.research/lab/checks | */.research/lab/checks/*) return 0 ;;
        */.research/lab)
            printf '%s' "$1" | grep -Eq "(^|[[:space:]'\"=])checks(/|[[:space:]'\"]|$)" && return 0
            ;;
    esac
    return 1
}

# Whether directory $1 holds stored runs: it is, or lies above as the
# project root, .research or .research/lab, a checks directory with runs.
holds_runs() {
    d=$(cd "$1" 2>/dev/null && pwd -P) || return 1
    case "$d" in */.research/lab/checks | */.research/lab/checks/*) return 0 ;; esac
    for c in "$d/.research/lab/checks" "$d/lab/checks" "$d/checks"; do
        case "$c" in */.research/lab/checks) ;; *) continue ;; esac
        for f in "$c"/*.jsonl; do
            [ -e "$f" ] && return 0
        done
    done
    return 1
}

if printf '%s' "$cmd" | grep -Eq '>[>|]?[[:space:]]*["'"'"']?[^[:space:];&|]*\.research/lab/checks'; then
    deny "a redirect into .research/lab/checks"
fi

parts=$(printf '%s\n' "$cmd" | sed -E 's/(&&|\|\||[;|])/\
/g')
# Prints what it refuses, for each part of the command on stdin.
scan() {
    while IFS= read -r part; do
        # After xargs, the paths come from an earlier part of the command.
        named=$part
        printf '%s' "$part" | grep -Eq '(^|[[:space:]])xargs([[:space:]]|$)' && named=$cmd
        # Drop wrappers before the command word.
        words=$(printf '%s' "$part" | sed -E 's/^[[:space:](!{]+//')
        i=0
        while [ $i -lt 4 ]; do
            words=$(printf '%s' "$words" | sed -E \
                -e 's/^(sudo|command|nohup|time|exec|env|xargs)([[:space:]]+-[^[:space:]]+)*[[:space:]]+//' \
                -e 's/^[A-Za-z_][A-Za-z0-9_]*=[^[:space:]]*[[:space:]]+//')
            i=$((i + 1))
        done
        set -f
        # shellcheck disable=SC2086
        set -- $words
        set +f
        [ $# -gt 0 ] || continue
        verb=${1##*/}
        shift
        dir=$cwd
        if [ "$verb" = git ]; then
            while [ $# -gt 0 ]; do
                case "$1" in
                    -C) [ $# -ge 2 ] || break
                        case "$2" in /*) dir=$2 ;; *) dir=$dir/$2 ;; esac
                        shift 2 ;;
                    -c) shift 2 ;;
                    -*) shift ;;
                    *) break ;;
                esac
            done
            [ $# -gt 0 ] || continue
            verb="git $1"
            shift
        fi
        case "$verb" in
            rm | rmdir | unlink | shred | truncate | mv | "git rm" | "git mv" | "git checkout" | "git clean") ;;
            "git restore")
                staged=0 worktree=0
                for w in "$@"; do
                    case "$w" in
                        --staged | -S) staged=1 ;;
                        --worktree | -W) worktree=1 ;;
                        --*) ;;
                        -*S*W* | -*W*S*) staged=1 worktree=1 ;;
                    esac
                done
                [ $staged = 1 ] && [ $worktree = 0 ] && continue ;;
            sed | perl)
                printf '%s' "$part" | grep -Eq '(^|[[:space:]])(-[a-zA-Z]*i|--in-place)' || continue ;;
            find)
                printf '%s' "$part" | grep -Eq '(^|[[:space:]])(-delete([[:space:]]|$)|-(exec|execdir|ok)[[:space:]]+[^[:space:]]*(rm|unlink|shred|mv|truncate)([[:space:]]|$))' || continue ;;
            *) continue ;;
        esac
        names_runs "$named" "$dir" && { echo "$verb"; break; }
        case "$verb" in
            rm | rmdir | "git rm")
                for w in "$@"; do
                    case "$w" in -*) continue ;; esac
                    w=${w#\'}; w=${w%\'}; w=${w#\"}; w=${w%\"}
                    case "$w" in /*) p=$w ;; *) p=$dir/$w ;; esac
                    if [ -d "$p" ] && holds_runs "$p"; then
                        echo "$verb $w"
                        break 2
                    fi
                done
                ;;
            "git clean")
                dry=0 ignored=--exclude-standard
                for w in "$@"; do
                    case "$w" in
                        --dry-run) dry=1 ;;
                        --*) ;;
                        -*n*) dry=1 ;;
                    esac
                    case "$w" in --*) ;; -*[xX]*) ignored= ;; esac
                done
                [ $dry = 1 ] && continue
                # shellcheck disable=SC2086
                found=$(git -C "$dir" ls-files --others $ignored -- ':(top)*.research/lab/checks/*.jsonl' 2>/dev/null | head -n 1)
                if [ -n "$found" ]; then
                    echo "git clean, with untracked run $found"
                    break
                fi
                ;;
        esac
    done
}
hit=$(printf '%s\n' "$parts" | scan)
[ -z "$hit" ] || deny "$hit"
exit 0
