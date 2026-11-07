#!/bin/sh
# Notify command for `lab loop`: files each rf.signal/1 (one JSON object on
# stdin) as a task on an agent-board board, or adds a note to the task filed
# for it before. Prints the task id, which lab keeps as the signal's ref.
#
#   [lab.loop]
#   notify = ["sh", ".claude/skills/research-factory/scripts/board-notify.sh", "--board", "<slug>"]
#
# Needs `board` (github.com/syz51/agent-board) and `jq` on PATH. The board
# identity comes from the environment of the session that started the loop.
set -eu

board_slug=""
while [ $# -gt 0 ]; do
    case "$1" in
        --board) board_slug="${2:?--board needs a value}"; shift 2 ;;
        *) echo "board-notify: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$board_slug" ] || { echo "board-notify: --board is required" >&2; exit 2; }
command -v board >/dev/null || { echo "board-notify: board is not on PATH" >&2; exit 2; }
command -v jq >/dev/null || { echo "board-notify: jq is not on PATH" >&2; exit 2; }

signal=$(cat)
field() { printf '%s' "$signal" | jq -r "$1"; }

schema=$(field '.schema // ""')
case "$schema" in
    rf.signal/1*) ;;
    *) echo "board-notify: expected an rf.signal/1 object, got schema '$schema'" >&2; exit 2 ;;
esac

kind=$(field '.kind')
id=$(field '.id')
title=$(field '.title')
project=$(field '.project')
ref=$(field '.ref // ""')
repeat=$(field '.repeat // false')
reopened=$(field '.reopened // false')

# A repeat of a signal already filed: add to that task instead of filing again.
if [ "$repeat" = true ] && [ "$reopened" != true ] && [ -n "$ref" ]; then
    note=$(printf '%s' "$signal" | jq -r '"finding: lab loop (\(.project)) again reports \(.id): \(.detail // "")" | .[0:500]')
    board note --kind finding "$ref" "$note" >/dev/null
    printf '%s\n' "$ref"
    exit 0
fi

lesson=$(field '.lesson // ""')
case "$kind" in
    lesson-worse)
        priority=p2
        accept="Lesson $lesson is revised (its rule changed and the change noted in its history) or retired with a note citing the evaluation
lab evaluate no longer reports $lesson as worse, or the lesson is retired" ;;
    measure-failed)
        priority=p2
        accept="The next lab loop run in $project measures without an error" ;;
    lesson-verifiable)
        priority=p3
        accept="lab lesson list shows $lesson as verified, set with a note citing the lab evaluate result" ;;
    lesson-unmeasurable)
        priority=p3
        accept="Lesson $lesson has a metric for which lab evaluate gives a verdict other than unmeasurable, or is verified or retired with --force and a note citing other evidence" ;;
    uncovered-rules)
        priority=p3
        accept="lab retro in $project lists no recurring rule without a lesson
Each rule that needs no practice has an observed lesson whose note says why" ;;
    *)
        priority=p3
        accept="The lab loop in $project no longer reports $id" ;;
esac

prefix=$(printf 'Signal `%s` from `lab loop` in project %s.' "$id" "$project")
if [ "$reopened" = true ] && [ -n "$ref" ]; then
    prefix="$prefix Reopened: it was filed before as $ref and went away."
fi
body=$(printf '%s' "$signal" | jq -r --arg pre "$prefix" '($pre + "\n\n" + .body) | .[0:4000]')
title=$(printf '%s' "$title" | jq -Rr '.[0:72]')
out=$(board add --json --board "$board_slug" --priority "$priority" --auto true \
    --body "$body" --accept "$accept" "$title")
task=$(printf '%s' "$out" | jq -r '.id // empty')
[ -n "$task" ] || { echo "board-notify: could not read the task id from: $out" >&2; exit 1; }
printf '%s\n' "$task"
