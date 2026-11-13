#!/bin/zsh
# dump all board events to events.jsonl
out=$1/events.jsonl; : > $out; since=0
while true; do
  board log --json --since $since --limit 5000 > $1/page.json || exit 1
  n=$(jq '.events|length' $1/page.json)
  [ "$n" = 0 ] && break
  jq -c '.events[]' $1/page.json >> $out
  since=$(jq '.next_since' $1/page.json)
done
wc -l $out
