#!/usr/bin/env bash
# verify.sh <pool dir>: for every "unique" row of each manifest, check the sha256 of the pool copy
# and of the live source in ~/.t3/worktrees/<repo>/<worktree>. Exit 1 when a copy mismatches.
set -u
P=${1:?usage: verify.sh <pool dir>}
[ -d "$P/manifests" ] || { echo "no manifests in $P" >&2; exit 2; }
ok=0; bad=0; srcchg=0
for m in "$P"/manifests/*.tsv; do
  b=$(basename "$m" .tsv); repo=${b%%--*}; wt=${b#*--}; src=~/.t3/worktrees/$repo/$wt
  while IFS=$'\t' read -r st size sha path; do
    [ "$st" = unique ] || continue
    c=$(shasum -a 256 "$P/$repo/$wt/$path" | cut -d' ' -f1)
    s=$(shasum -a 256 "$src/$path" | cut -d' ' -f1)
    if [ "$c" = "$sha" ]; then ok=$((ok+1)); else bad=$((bad+1)); echo "COPY MISMATCH $wt $path"; fi
    [ "$s" = "$sha" ] || { srcchg=$((srcchg+1)); echo "SOURCE CHANGED $wt $path"; }
  done < "$m"
done
echo "copies matching manifest: $ok, mismatched: $bad, sources changed since inventory: $srcchg"
[ "$bad" -eq 0 ]
