#!/usr/bin/env bash
# stage.sh <inventory dir> <worktree root> <stage dir> <repo name>
# Copies every "unique" row of each <inventory dir>/<worktree>.tsv from <worktree root>/<worktree>
# to <stage dir>/<repo name>/<worktree>/, and the manifest to <stage dir>/manifests/<repo name>--<worktree>.tsv.
# Then: storagectl put <stage dir> air:masters/  (the stage dir's name becomes the folder on air).
set -eu
INV=$1 ROOT=$2 ST=$3 REPO=$4
mkdir -p "$ST/manifests"
for f in "$INV"/*.tsv; do
  n=$(basename "$f" .tsv)
  cp "$f" "$ST/manifests/$REPO--$n.tsv"
  list=$(mktemp)
  awk -F'\t' '$1=="unique"{print $4}' "$f" > "$list"
  if [ -s "$list" ]; then rsync -a --files-from="$list" "$ROOT/$n/" "$ST/$REPO/$n/"; fi
  rm -f "$list"
done
