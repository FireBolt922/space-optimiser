#!/bin/sh
# The checks CI runs on each system, in one command: `npm run check`.
# Optional before a push; CI runs them on every pull request anyway.
set -eu
cd "$(dirname "$0")/.."

echo "== Prettier"
npx prettier --check .
echo "== Delete and process rules"
scripts/hygiene.sh
echo "== UI lint and tests"
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
echo "== Rust format, lint, and tests"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
echo "All checks passed."
