#!/bin/sh
# Claude Code PostToolUse hook: after a grep under a research archive, adds
# one line of context that names `lab search`. Off until you add it to a
# settings file:
#
#   "PostToolUse": [{ "matcher": "Grep|Bash", "hooks": [{ "type": "command",
#     "command": "sh .claude/skills/research-factory/scripts/grep-hint.sh" }] }]
#
# The archive root is the directory of the nearest research.toml, at or above
# the searched path, that configures `[lab.archive]` or `[lab.search]`. For
# any other path, any other command, or any error, it prints nothing. It
# never blocks: it always exits 0. Needs `jq` on PATH; without it, silent.
set -u

command -v jq >/dev/null 2>&1 || exit 0
input=$(cat) || exit 0
get() { printf '%s' "$input" | jq -r "$1" 2>/dev/null; }

tool=$(get '.tool_name // ""')
cwd=$(get '.cwd // ""')
[ -n "$cwd" ] || cwd=$(pwd)

case "$tool" in
    Grep)
        target=$(get '.tool_input.path // ""')
        ;;
    Bash)
        cmd=$(get '.tool_input.command // ""')
        # A grep, rg or ag as a command word: first, or after a pipe, `;` or `&&`.
        printf '%s\n' "$cmd" | grep -Eq '(^|[|;&(]|&&)[[:space:]]*(git[[:space:]]+grep|grep|egrep|rg|ag)([[:space:]]|$)' || exit 0
        # A leading `cd <dir> &&` moves the working directory.
        first=$(printf '%s' "$cmd" | sed -nE 's/^[[:space:]]*cd[[:space:]]+([^;&|[:space:]]+)[[:space:]]*&&.*/\1/p')
        case "$first" in
            "") ;;
            /*) cwd=$first ;;
            *) cwd=$cwd/$first ;;
        esac
        # The words of the last grep, up to the next pipe, `;` or `&`.
        args=$(printf '%s' "$cmd" | sed -E 's/.*(^|[|;&(])[[:space:]]*(git[[:space:]]+grep|egrep|grep|rg|ag)([[:space:]]|$)//')
        args=${args%%[|;&]*}
        # The searched path: the first of them that names an existing file or
        # directory; otherwise the working directory.
        target=""
        for word in $args; do
            case "$word" in -*|*'|'*|*';'*|*'&'*) continue ;; esac
            word=${word#\'}; word=${word%\'}; word=${word#\"}; word=${word%\"}
            case "$word" in /*) p=$word ;; *) p=$cwd/$word ;; esac
            if [ -e "$p" ]; then
                target=$p
                break
            fi
        done
        ;;
    *) exit 0 ;;
esac

case "$target" in
    "") target=$cwd ;;
    /*) ;;
    *) target=$cwd/$target ;;
esac
[ -d "$target" ] || target=$(dirname "$target")
dir=$(cd "$target" 2>/dev/null && pwd -P) || exit 0

# The nearest research.toml at or above the searched directory.
while :; do
    if [ -f "$dir/research.toml" ]; then
        grep -Eq '^[[:space:]]*\[\[?lab\.(archive|search)' "$dir/research.toml" 2>/dev/null || exit 0
        break
    fi
    [ "$dir" = "/" ] && exit 0
    dir=$(dirname "$dir")
done

msg="This grep ran in a research archive: \`lab search <words>\` ranks its notes and records and gives path#Lstart-Lend anchors (\`lab open <anchor>\` reads one); use it first for questions about past decisions and results."
jq -cn --arg m "$msg" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $m}}'
exit 0
