#!/usr/bin/env bash
# Measures what a compile costs when it renders all pages, and when it renders one page.
#
# lazytypst runs `typst compile --format png --diagnostic-format short --root <folder>` with
# `--pages <n>` for the page on screen. This script runs the same command for documents of
# different lengths, with and without `--pages`, and prints the time and the disk use.
#
# Usage: scripts/bench-pages.sh [PAGE COUNT]...
#        The default page counts are 1 50 200 500.
# Needs: typst in PATH.

set -euo pipefail

counts=("$@")
[ ${#counts[@]} -gt 0 ] || counts=(1 50 200 500)
runs=3

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Prints the median of the numbers on the lines of stdin.
median() { sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}'; }

# Prints the time in milliseconds that the command needs. Runs it $runs times and takes the median.
time_ms() {
  local i start
  for ((i = 0; i < runs; i++)); do
    rm -rf "$work/out" && mkdir "$work/out"
    start=$(date +%s%N)
    "$@" > /dev/null 2>&1
    echo $((($(date +%s%N) - start) / 1000000))
  done | median
}

echo "typst $(typst --version | awk '{print $2}'), median of $runs runs"
printf '%6s | %-26s | %-26s\n' pages "all pages (before)" "one page (after)"
printf '%6s | %8s %8s %7s | %8s %8s %7s\n' '' ms KiB files ms KiB files

for n in "${counts[@]}"; do
  cat > "$work/doc.typ" <<TYP
#set page(paper: "a5")
#for i in range($n) {
  if i > 0 { pagebreak() }
  heading[Page #(i + 1)]
  lorem(120)
}
TYP
  page=$(((n + 1) / 2)) # the page in the middle

  all=(typst compile --format png --diagnostic-format short --root "$work" "$work/doc.typ" "$work/out/page-{p}-of-{t}.png")
  one=(typst compile --format png --diagnostic-format short --root "$work" --pages "$page" "$work/doc.typ" "$work/out/page-{p}-of-{t}.png")

  all_ms=$(time_ms "${all[@]}")
  all_kib=$(du -sk "$work/out" | cut -f1)
  all_files=$(find "$work/out" -type f | wc -l)
  one_ms=$(time_ms "${one[@]}")
  one_kib=$(du -sk "$work/out" | cut -f1)
  one_files=$(find "$work/out" -type f | wc -l)

  # The document must have the page count that the row names.
  [ "$all_files" -eq "$n" ] || { echo "error: the document has $all_files pages, not $n" >&2; exit 1; }
  printf '%6s | %8s %8s %7s | %8s %8s %7s\n' "$n" "$all_ms" "$all_kib" "$all_files" "$one_ms" "$one_kib" "$one_files"
done
