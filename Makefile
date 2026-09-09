SHELL := /bin/bash

DESKTOP_DIR := native/apps/desktop
RUST_DIR := native

.PHONY: help dev run frontend rust-check check build build-windows build-mac build-linux mac-build install

help:
	@printf "Comandos disponíveis:\n"
	@printf "  make dev        Abre o app Tauri em desenvolvimento com hot reload\n"
	@printf "  make run        Executa o app Tauri localmente\n"
	@printf "  make frontend   Sobe somente o Vite em http://localhost:1420\n"
	@printf "  make rust-check Verifica o workspace Rust\n"
	@printf "  make check      Executa os checks do desktop e do SFU\n"
	@printf "  make build      Gera o build de produção do frontend\n"
	@printf "  make build-windows Gera NSIS e MSI para Windows\n"
	@printf "  make build-mac  Gera .app e .dmg para macOS\n"
	@printf "  make build-linux Gera DEB e AppImage para Linux\n"
	@printf "  make install     Instala e abre o .app no macOS\n"

dev:
	cd $(DESKTOP_DIR) && npm run tauri dev

run:
	cd $(DESKTOP_DIR) && npm run tauri dev -- --release

frontend:
	cd $(DESKTOP_DIR) && npm run dev

rust-check:
	cd $(RUST_DIR) && cargo check --workspace

check:
	cd $(DESKTOP_DIR) && npm run check
	cd sfu && npm run build
	$(MAKE) rust-check

build:
	cd $(DESKTOP_DIR) && npm run build

mac-build:
	cd $(DESKTOP_DIR) && npx tauri build --bundles app,dmg

build-windows:
	cd $(DESKTOP_DIR) && npx tauri build --bundles nsis,msi

build-mac: mac-build

build-linux:
	cd $(DESKTOP_DIR) && npx tauri build --bundles deb,appimage

install: mac-build
	@set -e; \
	app="$(CURDIR)/native/target/release/bundle/macos/Unkvoid.app"; \
	if pgrep -x Unkvoid >/dev/null 2>&1; then \
		osascript -e 'tell application "Unkvoid" to quit' >/dev/null 2>&1 || true; \
		sleep 2; \
	fi; \
	rm -rf /Applications/Unkvoid.app; \
	ditto "$$app" /Applications/Unkvoid.app; \
	open -a /Applications/Unkvoid.app
