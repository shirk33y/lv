#!/bin/bash
# multi.sh — run commands in parallel with colored streaming output.
#
# Commands are read from stdin, separated by \0 (NUL).
# Each command gets a short label derived from its first word.
#
# Usage:
#   printf '%s\0' 'cargo test' 'cargo clippy -- -D warnings' | bash scripts/multi.sh
#   echo -ne 'sleep 1\0echo hi' | bash scripts/multi.sh
set -eo pipefail

# ── Colors ────────────────────────────────────────────────────────────
RST="\033[0m"
BOLD="\033[1m"
RED="\033[31m"
GRN="\033[32m"
PALETTE=("\033[32m" "\033[33m" "\033[36m" "\033[35m" "\033[34m" "\033[91m" "\033[92m" "\033[96m")

# ── Read NUL-separated commands from stdin ────────────────────────────
cmds=()
while IFS= read -r -d '' cmd; do
  [[ -n "$cmd" ]] && cmds+=("$cmd")
done

if [[ ${#cmds[@]} -eq 0 ]]; then
  echo "multi.sh: no commands received (pipe NUL-separated commands to stdin)" >&2
  exit 1
fi

# ── Derive short labels (for log prefix) ─────────────────────────────
labels=()
for cmd in "${cmds[@]}"; do
  label=$(echo "$cmd" | awk '{print $2}')
  [[ -z "$label" ]] && label=$(echo "$cmd" | awk '{print $1}')
  labels+=("$label")
done

# ── Logging ───────────────────────────────────────────────────────────
log() {
  local label="$1" color="$2"
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    printf "${color}%-7s${RST} │ %s\n" "$label" "$line"
  done
}

# ── Run in parallel ──────────────────────────────────────────────────
pids=()

echo -e "${BOLD}Running ${#cmds[@]} commands in parallel...${RST}"
echo

for i in "${!cmds[@]}"; do
  color="${PALETTE[$((i % ${#PALETTE[@]}))]}"
  (
    bash -c "${cmds[$i]}" 2>&1 | log "${labels[$i]}" "$color"
    exit ${PIPESTATUS[0]}
  ) &
  pids+=($!)
done

# ── Wait and collect results ─────────────────────────────────────────
failed=0
declare -A results

for i in "${!cmds[@]}"; do
  if wait "${pids[$i]}" 2>/dev/null; then
    results[$i]="ok"
  else
    results[$i]="FAIL"
    failed=$((failed + 1))
  fi
done

# ── Report ───────────────────────────────────────────────────────────
echo
DIM="\033[2m"
# Failed first
for i in "${!cmds[@]}"; do
  [[ "${results[$i]}" == "ok" ]] && continue
  cmd="${cmds[$i]}"
  if [[ "$cmd" == *" -- "* ]]; then
    main="${cmd%% -- *}"
    args="${cmd#* -- }"
    echo -e "  ${RED}✘${RST} ${main} ${DIM}-- ${args}${RST}"
  else
    echo -e "  ${RED}✘${RST} ${cmd}"
  fi
done
for i in "${!cmds[@]}"; do
  [[ "${results[$i]}" != "ok" ]] && continue
  cmd="${cmds[$i]}"
  if [[ "$cmd" == *" -- "* ]]; then
    main="${cmd%% -- *}"
    args="${cmd#* -- }"
    echo -e "  ${GRN}✓${RST} ${main} ${DIM}-- ${args}${RST}"
  else
    echo -e "  ${GRN}✓${RST} ${cmd}"
  fi
done

echo
if [[ $failed -gt 0 ]]; then
  exit 1
fi
