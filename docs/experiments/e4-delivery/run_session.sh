#!/bin/sh
# Runs one E4 session: one topic under one delivery condition, headless, with
# Claude Opus 5.5 through the `claude` CLI, in a scratch copy of masters.
#
#   run_session.sh <condition> <topic id>
#
# Environment:
#   E4_MASTERS  the scratch copy of masters (the working directory of the session)
#   E4_TESTSET  the test set folder (read for the topic's `need` only)
#   E4_OUT      where the transcript goes: <out>/<condition>/<topic>.jsonl
#   E4_LAB      the directory of the `lab` binary under test
#   E4_SKILL    the skill folder of this repository (skill/research-factory)
#
# Conditions:
#   cli      the skill teaches `lab search`; no other delivery
#   mcp      cli + `lab mcp` as an MCP server (archive_search, archive_open, archive_related)
#   hint     cli + the grep-hint PostToolUse hook
#   agent    cli + the archivist subagent
set -eu
cond=$1
topic=$2
: "${E4_MASTERS:?}" "${E4_TESTSET:?}" "${E4_OUT:?}" "${E4_LAB:?}" "${E4_SKILL:?}"

need=$(jq -r --arg id "$topic" 'select(.id == $id) | .need' "$E4_TESTSET/topics.jsonl")
[ -n "$need" ] || { echo "no topic $topic" >&2; exit 2; }
prompt="Answer from this project's research archive. Find: $need
Give a short answer, then the path:start-end anchor of each passage it rests on. Do not change any file."

empty_mcp='{"mcpServers":{}}'
mcp=$empty_mcp
settings='{}'
agents='{}'
case "$cond" in
    cli) ;;
    mcp) mcp=$(jq -cn --arg lab "$E4_LAB/lab" '{mcpServers: {lab: {type: "stdio", command: $lab, args: ["mcp"]}}}') ;;
    hint) settings=$(jq -cn --arg s "$E4_SKILL/scripts/grep-hint.sh" \
        '{hooks: {PostToolUse: [{matcher: "Grep|Bash", hooks: [{type: "command", command: ("sh " + $s)}]}]}}') ;;
    agent)
        # The archivist definition: front matter fields and the body as its prompt.
        def=$E4_SKILL/agents/archivist.md
        desc=$(sed -n 's/^description: //p' "$def")
        body=$(awk 'n >= 2 {print} /^---$/ {n++}' "$def")
        agents=$(jq -cn --arg d "$desc" --arg p "$body" \
            '{archivist: {description: $d, prompt: $p, tools: ["Bash", "Read"], model: "inherit"}}')
        ;;
    *) echo "unknown condition $cond" >&2; exit 2 ;;
esac

mkdir -p "$E4_OUT/$cond"
out=$E4_OUT/$cond/$topic.jsonl
cd "$E4_MASTERS"
PATH=$E4_LAB:$PATH claude -p "$prompt" \
    --model claude-opus-5-5 \
    --setting-sources project \
    --strict-mcp-config --mcp-config "$mcp" \
    --settings "$settings" \
    --agents "$agents" \
    --allowedTools "Bash Read Grep Glob Skill Task Agent mcp__lab__archive_search mcp__lab__archive_open mcp__lab__archive_related" \
    --disallowedTools "Write Edit NotebookEdit WebFetch WebSearch" \
    --max-turns 40 \
    --no-session-persistence \
    --output-format stream-json --verbose \
    >"$out" 2>"$out.err" || echo "exit $? for $cond $topic" >&2
