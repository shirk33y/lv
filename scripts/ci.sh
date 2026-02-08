#!/bin/bash
# ci.sh — run all checks in parallel with colored streaming output
set -eo pipefail

cd "$(dirname "$0")/.."

# ── Colors ────────────────────────────────────────────────────────────
RST="\033[0m"
BOLD="\033[1m"
DIM="\033[2m"
RED="\033[31m"
GRN="\033[32m"
YLW="\033[33m"
BLU="\033[34m"
MAG="\033[35m"
CYN="\033[36m"

COLORS=("$GRN" "$YLW" "$CYN")
NAMES=("test" "clippy" "fmt")
CMDS=(
  "cargo test"
  "cargo clippy -- -D warnings"
  "cargo fmt -- --check"
)

# ── Logging ───────────────────────────────────────────────────────────
log() {
  local name="$1" color="$2"
  while IFS= read -r line; do
    # skip noise: blank lines, progress bars, notes, warnings preamble
    [[ -z "$line" ]] && continue
    [[ "$line" == *"Compiling"* ]] && continue
    [[ "$line" == *"Checking"* ]] && continue
    [[ "$line" == *"Blocking"* ]] && continue
    [[ "$line" == *"Finished"* ]] && continue
    [[ "$line" == *"Running"* ]] && continue
    [[ "$line" == *"Building"* ]] && continue
    printf "${color}%-7s${RST} │ %s\n" "$name" "$line"
  done
}

# ── Run checks in parallel ───────────────────────────────────────────
pids=()
tmpdir=$(mktemp -d)
trap "rm -rf $tmpdir" EXIT

echo -e "${BOLD}Running ${#CMDS[@]} checks in parallel...${RST}"
echo

for i in "${!CMDS[@]}"; do
  (
    ${CMDS[$i]} 2>&1 | log "${NAMES[$i]}" "${COLORS[$i]}"
    exit ${PIPESTATUS[0]}
  ) &
  pids+=($!)
  echo "$!" > "$tmpdir/${NAMES[$i]}.pid"
done

# ── Wait and collect results ─────────────────────────────────────────
declare -A results
failed=0

for i in "${!NAMES[@]}"; do
  if wait "${pids[$i]}" 2>/dev/null; then
    results[${NAMES[$i]}]="ok"
  else
    results[${NAMES[$i]}]="FAIL"
    failed=$((failed + 1))
  fi
done

# ── Report ───────────────────────────────────────────────────────────
echo
echo -e "${MAG}checks:${RST}"

# failed first
for name in "${NAMES[@]}"; do
  [[ "${results[$name]}" == "ok" ]] && continue
  echo -e "  ${RED}✘${RST} ${name}"
done
for name in "${NAMES[@]}"; do
  [[ "${results[$name]}" != "ok" ]] && continue
  echo -e "  ${GRN}✓${RST} ${name}"
done

echo
if [[ $failed -gt 0 ]]; then
  exit 1
fi
