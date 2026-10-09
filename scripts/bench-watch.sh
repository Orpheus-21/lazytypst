#!/usr/bin/env bash
# Measures what a new `typst compile` costs for one page, and what `typst watch` needs for the same
# change. lazytypst starts a new typst process for each compile. A running `typst watch` keeps its caches.
#
# For each page count the script makes a document, runs `typst compile --pages 1` (the command of
# lazytypst) a few times, and then starts `typst watch --pages 1`, changes the file, and measures the
# time until the new image is written. It prints the median of each.
#
# Usage: scripts/bench-watch.sh [PAGE COUNT]...
#        The default page counts are 10 100 500.
# Needs: typst in PATH.

set -euo pipefail

counts=("$@")
[ ${#counts[@]} -gt 0 ] || counts=(10 100 500)
runs=5

work=$(mktemp -d)
watch_pid=
cleanup() { [ -z "$watch_pid" ] || kill "$watch_pid" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

median() { sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}'; }
now_ms() { echo $(($(date +%s%N) / 1000000)); }

echo "typst $(typst --version | awk '{print $2}'), median of $runs runs"
printf '%6s | %12s | %12s | %s\n' pages 'compile (ms)' 'watch (ms)' 'watch / compile'

for n in "${counts[@]}"; do
  doc="$work/doc.typ"
  cat > "$doc" <<TYP
#set page(width: 12cm, height: 8cm)
#let tag = "start"
#for i in range($n) [
  = Page #i #tag
  #lorem(40)
  #pagebreak()
]
TYP

  # A new process for each compile, as lazytypst does.
  for ((i = 0; i < runs; i++)); do
    rm -f "$work"/out-*.png
    start=$(now_ms)
    typst compile --format png --diagnostic-format short --root "$work" --pages 1 "$doc" "$work/out-{p}-of-{t}.png" > /dev/null 2>&1
    echo $(($(now_ms) - start))
  done > "$work/compile.txt"

  # One long process. Each change of the text changes the first page, so it must make a new image.
  rm -f "$work"/out-*.png
  typst watch --format png --diagnostic-format short --root "$work" --pages 1 "$doc" "$work/out-{p}-of-{t}.png" > /dev/null 2>&1 &
  watch_pid=$!
  for _ in $(seq 400); do ls "$work"/out-*.png > /dev/null 2>&1 && break; sleep 0.05; done
  for ((i = 0; i < runs; i++)); do
    before=$(stat -c %Y.%y "$work"/out-*.png)
    start=$(now_ms)
    sed -i "s/^#let tag = .*/#let tag = \"run $i\"/" "$doc"
    for _ in $(seq 2000); do
      [ "$(stat -c %Y.%y "$work"/out-*.png 2> /dev/null)" != "$before" ] && break
      sleep 0.002
    done
    echo $(($(now_ms) - start))
  done > "$work/watch.txt"
  kill "$watch_pid" 2> /dev/null || true
  wait "$watch_pid" 2> /dev/null || true
  watch_pid=

  c=$(median < "$work/compile.txt")
  w=$(median < "$work/watch.txt")
  printf '%6s | %12s | %12s | %s\n' "$n" "$c" "$w" "$(awk -v c="$c" -v w="$w" 'BEGIN {printf "%.2f", w / c}')"
done
