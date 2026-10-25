SHELL := /bin/bash

NATIVE_DIRECTORY := native
RELEASE_DIRECTORY := $(NATIVE_DIRECTORY)/release

.PHONY: help rust-check build-windows build-linux build-vps release publish-windows

VPS_REMOTE ?= vps
VPS_APPLICATION := /var/www/projects/unkvoid
WINDOWS_INSTALLERS ?= /mnt/c/Users/edsu/Desktop/apps
RELEASE_SECRET_FILE ?= $(HOME)/auxilos/release-secret.env
VERSION = $(shell awk '\
  /^\[workspace\.package\]$$/ { in_workspace_package = 1; next } \
  /^\[/ { in_workspace_package = 0 } \
  in_workspace_package && /^version[[:space:]]*=/ { value = $$0; sub(/^[^"]*"/, "", value); sub(/".*$$/, "", value); print value; exit } \
' $(NATIVE_DIRECTORY)/Cargo.toml)

help:
	@printf "Comandos disponíveis:\n"
	@printf "  make rust-check       Verifica o workspace Rust\n"
	@printf "  make build-windows    Gera o instalador .exe no Windows\n"
	@printf "  make build-linux      Gera o .deb do app único no Debian 12\n"
	@printf "  make build-vps        Gera e publica o .deb no APT pela VPS\n"
	@printf "  make release          Publica os instaladores desta máquina no GitHub\n"
	@printf "  make publish-windows  Publica o .exe do Windows no site\n"

rust-check:
	cd $(NATIVE_DIRECTORY) && cargo check --workspace

build-windows:
	powershell.exe -ExecutionPolicy Bypass -File native\apps\unkvoid\build-installer.ps1

build-linux:
	cd $(NATIVE_DIRECTORY)/apps/unkvoid && ./build-deb.sh

# Por rsync e não por git: a VPS não precisa guardar uma credencial do GitHub para compilar.
build-vps:
	@set -e; \
	ssh $(VPS_REMOTE) "mkdir -p $(VPS_APPLICATION)"; \
	rsync -az --delete \
		--exclude .git --exclude node_modules --exclude target --exclude dist \
		./native ./Makefile "$(VPS_REMOTE):$(VPS_APPLICATION)/"; \
	ssh $(VPS_REMOTE) "$(VPS_APPLICATION)/native/release/build-vps.sh"

release:
	node $(RELEASE_DIRECTORY)/release.mjs

# O segredo fica no WSL, e o instalador é assinado antes de chegar à API do site.
publish-windows:
	@set -e; \
	source $(RELEASE_SECRET_FILE); \
	export RELEASE_SECRET; \
	for installer in $(WINDOWS_INSTALLERS)/Unkvoid_$(VERSION)_*-setup.exe; do \
		$(RELEASE_DIRECTORY)/publish-release.sh windows-x86_64-nsis "$$installer" "$$installer.sig"; \
	done
