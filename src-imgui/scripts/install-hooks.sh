#!/bin/bash
# Install git pre-commit hook that runs CI checks before each commit.
# Usage: bash scripts/install-hooks.sh
set -euo pipefail

HOOK="$(git -C "$(dirname "$0")/.." rev-parse --git-dir)/hooks/pre-commit"

cat > "$HOOK" << 'EOF'
#!/bin/bash
# pre-commit hook — runs cargo test + clippy + fmt check
# Installed by: bash src-imgui/scripts/install-hooks.sh
set -eo pipefail

cd "$(git rev-parse --show-toplevel)/src-imgui"

echo "pre-commit: running checks..."

cargo fmt -- --check 2>&1 || {
  echo "❌ cargo fmt failed — run 'cargo fmt' to fix"
  exit 1
}

cargo clippy -- -D warnings 2>&1 || {
  echo "❌ clippy failed"
  exit 1
}

cargo test 2>&1 || {
  echo "❌ tests failed"
  exit 1
}

echo "✓ all checks passed"
EOF

chmod +x "$HOOK"
echo "Installed pre-commit hook: $HOOK"
