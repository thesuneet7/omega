# Omega — Electron + Vite + Rust (omega-api + sensor_layer CLI).
# Requires: Rust toolchain, Node.js 18+.

.PHONY: help install-ui desktop desktop-clean dev capture omega-api

help:
	@echo "Targets:"
	@echo "  make install-ui      npm ci in ui/"
	@echo "  make desktop         npm ci in ui/ then Electron + Vite + omega-api"
	@echo "  make desktop-clean   Isolated temp logs/db then desktop (see script)"
	@echo "  make omega-api       Local HTTP API only (default cargo run)"
	@echo "  make capture         Phase 1 sensor CLI (cargo run --bin sensor_layer)"
	@echo ""
	@echo "Web UI without Electron: terminal 1) make omega-api  terminal 2) cd ui && npm run dev"

install-ui:
	cd ui && npm ci

desktop: install-ui
	cd ui && npm run electron:dev

desktop-clean:
	bash scripts/run-desktop-clean.sh

dev:
	@echo "Run in two terminals: make omega-api   and   cd ui && npm run dev"

omega-api:
	cargo run --bin omega-api

capture:
	cargo run --bin sensor_layer
