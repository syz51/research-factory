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
# - A Bash command part (split at `;`, `&&`, `||`, `|` and newlines outside
#   quotes) that runs rm, rmdir, unlink, shred, truncate, mv, `sed -i`,
#   `perl -i`, `find -delete`, `find -exec rm`, or git rm, mv, clean, checkout
#   or restore (not `restore --staged` alone), and names a path under
#   .research/lab/checks (or `checks/` from .research/lab). After `xargs`, a
#   path named anywhere in the command counts.
# - cp, install or ln into .research/lab/checks, tee or `dd of=` on it, and a
#   redirect (`>` or `>>`) into it.
# - rm, rmdir or `git rm` of a directory that holds stored runs (the checks
#   directory, .research/lab, .research, or a project root that has them),
#   also through a glob such as `.research/lab/*`.
# - `find -delete` or `find -exec rm` from such a directory, unless its -name
#   or -path tests cannot match a run, or it has `-type d`.
# - `git clean` without -n or --dry-run while the repository has untracked
#   runs (with -x or -X, ignored runs count too).
# A leading `cd <dir> &&` or `cd <dir>;` moves the directory that relative
# paths start from; `~` is the home directory. It reads commands as text: a
# command that builds the path at run time, or deletes a directory further
# above the project, passes. It is a guard against mistakes, not a security
# boundary. Anything else, any parse error, or no `jq` on PATH: it allows the
# call. It always exits 0.
set -u

command -v jq >/dev/null 2>&1 || exit 0
# One jq call reads the tool, its directory and its command or path, one per
# line; the command or path is last, as it can hold newlines. The `.` keeps
# empty lines at the end.
nl='
'
fields=$(jq -r '.tool_name // "", .cwd // "", .tool_input.command // .tool_input.file_path // .tool_input.notebook_path // ""' 2>/dev/null && echo .) || exit 0
fields=${fields%"$nl".}
tool=${fields%%"$nl"*}
fields=${fields#*"$nl"}
cwd=${fields%%"$nl"*}
arg=${fields#*"$nl"}

deny() {
    msg="Refused: this would delete or edit a stored check run ($1). Stored check runs in .research/lab/checks/ are never deleted or edited (research-factory skill, \"Before reporting done\", step 1). A rerun supersedes an earlier run without removing it: commit every run of the session, superseded ones included. Only a person may remove one, outside the agent."
    jq -cn --arg m "$msg" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $m}}'
    exit 0
}

[ -n "$cwd" ] || cwd=$(pwd)

case "$tool" in
    Edit | Write | MultiEdit | NotebookEdit)
        case "$arg" in
            */.research/lab/checks/* | .research/lab/checks/*) deny "$tool $arg" ;;
        esac
        exit 0
        ;;
    Bash) ;;
    *) exit 0 ;;
esac

cmd=$arg
[ -n "$cmd" ] || exit 0
# Each refusal below needs one of these words as a command word (also after a
# `/`), or names .research/lab/checks, so most commands stop here.
printf '%s\n' "$cmd" | grep -Eq '(^|[[:space:];|&<>/(!{])(rm|rmdir|unlink|shred|truncate|mv|git|sed|perl|find|cp|install|ln|tee|dd)([[:space:];|&<>]|$)|\.research/lab/checks' || exit 0

# The command with `;`, `|`, `&`, `<` and `>` inside quotes replaced by
# spaces, so that quoted text is not split or read as a redirect.
masked=$(printf '%s\n' "$cmd" | awk -v sq="'" '{
    out = ""
    for (i = 1; i <= length($0); i++) {
        c = substr($0, i, 1)
        if (q == "") { if (c == sq || c == "\"") q = c }
        else if (c == q) q = ""
        else if (index(";|&<>", c)) c = " "
        out = out c
    }
    print out
}')

# $1 without surrounding quotes, with `~` expanded, absolute from $2.
abs() {
    a=${1#\'}; a=${a%\'}; a=${a#\"}; a=${a%\"}
    case "$a" in
        \~) a=$HOME ;;
        \~/*) a=$HOME/${a#\~/} ;;
    esac
    case "$a" in /*) printf '%s' "$a" ;; *) printf '%s' "$2/$a" ;; esac
}

# A leading `cd <dir> &&` or `cd <dir>;` moves the working directory.
first=$(printf '%s' "$masked" | sed -nE '1s/^[[:space:]]*cd[[:space:]]+([^;&|[:space:]]+)[[:space:]]*(&&|;).*/\1/p')
[ -z "$first" ] || cwd=$(abs "$first" "$cwd")

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

# Whether path $1 is a stored run or a directory that holds stored runs: a
# checks directory with runs, or .research/lab, .research or a project root
# above one. Sets `run` to one stored run below it, and `d` to $1 without
# links. Leaves pathname expansion off, as `scan` has it.
d="" run=""
holds_runs() {
    case "$1" in */.research/lab/checks/*) run=$1; return 0 ;; esac
    d=$(cd "$1" 2>/dev/null && pwd -P) || return 1
    set +f
    for c in "$d" "$d/.research/lab/checks" "$d/lab/checks" "$d/checks"; do
        case "$c" in */.research/lab/checks) ;; *) continue ;; esac
        for f in "$c"/*.jsonl; do
            [ -e "$f" ] && { run=$f; set -f; return 0; }
        done
    done
    set -f
    return 1
}

if printf '%s' "$masked" | grep -Eq '>[>|]?[[:space:]]*["'"'"']?[^[:space:];&|]*\.research/lab/checks'; then
    deny "a redirect into .research/lab/checks"
fi

parts=$(printf '%s\n' "$masked" | sed -E 's/(&&|\|\||[;|])/\
/g')
# Prints what it refuses, for each part of the command on stdin.
scan() {
    while IFS= read -r part; do
        # After xargs, the paths come from an earlier part of the command.
        named=$part
        printf '%s' "$part" | grep -Eq '(^|[[:space:]])xargs([[:space:]]|$)' && named=$masked
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
        [ $# -gt 0 ] || continue
        verb=${1##*/}
        shift
        dir=$cwd
        if [ "$verb" = git ]; then
            while [ $# -gt 0 ]; do
                case "$1" in
                    -C) [ $# -ge 2 ] || break
                        dir=$(abs "$2" "$dir")
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
            cp | install | ln)
                # The destination is the last operand.
                dest=""
                for w in "$@"; do
                    case "$w" in -*) ;; *) dest=$w ;; esac
                done
                [ -n "$dest" ] && names_runs "$dest" "$dir" && { echo "$verb into $dest"; break; }
                continue ;;
            tee)
                # Its files are its operands; not what a `<` reads from.
                prev=""
                for w in "$@"; do
                    case "$prev" in "<") prev=$w; continue ;; esac
                    prev=$w
                    case "$w" in -* | "<"*) continue ;; esac
                    names_runs "$w" "$dir" && { echo "tee $w"; break 2; }
                done
                continue ;;
            dd)
                for w in "$@"; do
                    case "$w" in of=*) names_runs "${w#of=}" "$dir" && { echo "dd $w"; break 2; } ;; esac
                done
                continue ;;
            *) continue ;;
        esac
        names_runs "$named" "$dir" && { echo "$verb"; break; }
        case "$verb" in
            rm | rmdir | "git rm")
                for w in "$@"; do
                    case "$w" in -*) continue ;; esac
                    p=$(abs "$w" "$dir")
                    # A glob is expanded here as the shell would; no field splitting.
                    set +f
                    old_ifs=$IFS
                    IFS=
                    for e in $p; do
                        if holds_runs "$e"; then
                            IFS=$old_ifs
                            echo "$verb $w"
                            break 3
                        fi
                    done
                    IFS=$old_ifs
                    set -f
                done
                ;;
            find)
                # Start points: the operands before the first test; `.` by default.
                starts="" tests=""
                for w in "$@"; do
                    if [ -z "$tests" ]; then
                        case "$w" in -* | '(' | '!' | '\(' | '\!') tests=1 ;; *) starts="$starts $w" ;; esac
                    fi
                done
                [ -n "$starts" ] || starts=.
                printf '%s' "$part" | grep -Eq '(^|[[:space:]])-type[[:space:]]+d([[:space:]]|$)' && continue
                for s in $starts; do
                    holds_runs "$(abs "$s" "$dir")" || continue
                    # The run as find would name it.
                    shown=${s%/}${run#"$d"}
                    base=${run##*/}
                    matched="" filtered=""
                    prev=""
                    for w in "$@"; do
                        pat=${w#\'}; pat=${pat%\'}; pat=${pat#\"}; pat=${pat%\"}
                        case "$prev" in
                            -name | -iname)
                                filtered=1
                                # shellcheck disable=SC2254
                                case "$base" in $pat) matched=1 ;; esac ;;
                            -path | -ipath | -wholename | -iwholename)
                                filtered=1
                                # shellcheck disable=SC2254
                                case "$shown" in $pat) matched=1 ;; esac ;;
                            -regex | -iregex) filtered=1 matched=1 ;;
                        esac
                        prev=$w
                    done
                    if [ -z "$filtered" ] || [ -n "$matched" ]; then
                        echo "find from $s, which holds stored runs"
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
