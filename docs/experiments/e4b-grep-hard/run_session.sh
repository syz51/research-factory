#!/bin/sh
# Runs one E4b session: one topic under one condition, replicate <rep>, headless,
# with Claude Opus 5.5 through the `claude` CLI, in a scratch copy of masters.
# The flags are E4's (../e4-delivery/run_session.sh); each condition has its own copy.
#
#   run_session.sh <condition> <topic id> <rep> [phrasing]
#
# Without [phrasing] the prompt asks for the topic's `need` (round 1); with 1 or 2 it
# asks for that entry of the topic's `phrasings` (round 2).
#
# Environment:
#   E4B_MASTERS  a folder with two scratch copies of masters: masters-cli (no .mcp.json,
#                so `lab brief` teaches `lab search`) and masters-mcp (.mcp.json runs
#                `lab mcp`, so the brief points to `archive_search`)
#   E4B_TESTSET  the test set folder (read for the topic's `need` or `phrasings` only)
#   E4B_OUT      where the transcript goes: <out>/<condition>/<topic>-<rep>.jsonl
#   E4B_LAB      the directory of the `lab` binary under test
#
# Conditions:
#   cli   the skill and `lab brief` teach `lab search`; no MCP server
#   mcp   cli + `lab mcp` as an MCP server (archive_search, archive_open, archive_related)
set -eu
cond=$1
topic=$2
rep=$3
phrasing=${4:-}
: "${E4B_MASTERS:?}" "${E4B_TESTSET:?}" "${E4B_OUT:?}" "${E4B_LAB:?}"

if [ -n "$phrasing" ]; then
    need=$(jq -r --arg id "$topic" --argjson k "$phrasing" 'select(.id == $id) | .phrasings[$k - 1]' "$E4B_TESTSET/topics.jsonl")
else
    need=$(jq -r --arg id "$topic" 'select(.id == $id) | .need' "$E4B_TESTSET/topics.jsonl")
fi
[ -n "$need" ] && [ "$need" != null ] || { echo "no topic $topic" >&2; exit 2; }
prompt="Answer from this project's research archive. Find: $need
Give a short answer, then the path:start-end anchor of each passage it rests on. Do not change any file."

case "$cond" in
    cli) mcp='{"mcpServers":{}}' ;;
    mcp) mcp=$(jq -cn --arg lab "$E4B_LAB/lab" '{mcpServers: {lab: {type: "stdio", command: $lab, args: ["mcp"]}}}') ;;
    *) echo "unknown condition $cond" >&2; exit 2 ;;
esac

mkdir -p "$E4B_OUT/$cond"
out=$E4B_OUT/$cond/$topic-$rep.jsonl
cd "$E4B_MASTERS/masters-$cond"
echo "$(date -u +%FT%TZ) start $cond $topic $rep" >>"$E4B_OUT/order.log"
# AGENT_BOARD_HOOKS=off: the session is a nested `claude -p`, not a board agent.
AGENT_BOARD_HOOKS=off PATH=$E4B_LAB:$PATH claude -p "$prompt" \
    --model claude-opus-5-5 \
    --setting-sources project \
    --strict-mcp-config --mcp-config "$mcp" \
    --settings '{}' \
    --agents '{}' \
    --allowedTools "Bash Read Grep Glob Skill Task Agent mcp__lab__archive_search mcp__lab__archive_open mcp__lab__archive_related" \
    --disallowedTools "Write Edit NotebookEdit WebFetch WebSearch" \
    --max-turns 40 \
    --no-session-persistence \
    --output-format stream-json --verbose \
    >"$out" 2>"$out.err" || echo "exit $? for $cond $topic $rep" >&2
