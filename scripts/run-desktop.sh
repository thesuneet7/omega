#!/usr/bin/env bash
# Deterministic desktop startup: install UI deps from lockfile, then Electron + Vite + Rust build.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/ui"
npm ci
npm run electron:dev
