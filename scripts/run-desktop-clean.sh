#!/usr/bin/env bash
# One isolated "run": empty capture list, fresh app DB, fresh phase DB until you quit the app.
# Uses the same env for the desktop UI and for Phase 1 capture (OMEGA_APP_LOGS_DIR).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN_DIR="${OMEGA_RUN_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/omega-run.XXXXXX")}"
mkdir -p "$RUN_DIR"

export OMEGA_APP_LOGS_DIR="$RUN_DIR"
export OMEGA_PHASE2_DB_PATH="${OMEGA_PHASE2_DB_PATH:-$RUN_DIR/phase2.db}"
export OMEGA_PHASE3_DB_PATH="${OMEGA_PHASE3_DB_PATH:-$RUN_DIR/phase2.db}"
export OMEGA_PHASE4_DB_PATH="${OMEGA_PHASE4_DB_PATH:-$RUN_DIR/phase2.db}"

echo "Omega isolated run directory: $RUN_DIR"
echo "(Set OMEGA_RUN_DIR to reuse a folder; unset to create a new temp dir each time.)"

cd "$ROOT/ui"
exec npm run electron:dev
