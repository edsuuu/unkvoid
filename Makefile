SHELL := /bin/bash

DESKTOP_DIR := native/apps/desktop
RUST_DIR := native

.PHONY: help dev run frontend rust-check check build build-windows build-mac build-linux mac-build install build-vps release publish-windows

VPS_REMOTE ?= vps
VPS_APP := /var/www/projects/unkvoid
REPO_URL := https://github.com/edsuuu/unkvoid.git
CHAVE := $(HOME)/.tauri/unkvoid.key

# Onde o build-windows.ps1 larga os instaladores, visto de dentro do WSL.
WINDOWS_APPS ?= /mnt/c/Users/edsu/Desktop/apps
VERSAO = $(shell node -pe 'JSON.parse(require("fs").readFileSync("$(DESKTOP_DIR)/src-tauri/tauri.conf.json","utf8")).version')

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
	@printf "  make build-vps   Gera DEB e AppImage na VPS e publica assinado\n"
	@printf "  make release     Publica os instaladores desta máquina na release\n"
	@printf "  make publish-windows Publica o .msi e o .exe que o Windows acabou de gerar\n"

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

# A chave do updater entra aqui. Sem ela o bundle do .app.tar.gz para no fim do
# build pedindo senha, e o erro derruba o `make install` antes de instalar.
mac-build:
	cd $(DESKTOP_DIR) && TAURI_SIGNING_PRIVATE_KEY="$$(cat $(CHAVE))" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npx tauri build --bundles app,dmg

build-windows:
	cd $(DESKTOP_DIR) && npx tauri build --bundles nsis,msi

build-mac: mac-build

build-linux:
	cd $(DESKTOP_DIR) && npx tauri build --bundles deb,appimage

# O Linux sai da VPS: gera o .deb e publica no APT, que é como o Linux instala e
# atualiza. O .dmg e o .msi saem das máquinas de cada sistema, porque nenhum dos dois
# pode ser gerado em Linux — um exige um Mac por licença, o outro exige o WiX no Windows.
#
# A chave que assina atualizações NÃO vem para cá. O Linux não a usa, e a VPS é uma
# máquina exposta: quem a tiver publica atualização para todo mundo que instalou o app.
#
# Por rsync e não por git: o repositório é privado, e clonar de lá exigiria uma
# credencial do GitHub guardada numa máquina exposta à internet. O `--delete` mantém a
# cópia igual à daqui, e os `--exclude` preservam o cache do cargo, que são 2 GB e
# quarenta minutos de compilação.
build-vps:
	@set -e; \
	ssh $(VPS_REMOTE) "mkdir -p $(VPS_APP)"; \
	rsync -az --delete \
		--exclude .git --exclude node_modules --exclude target --exclude dist \
		./native ./Makefile "$(VPS_REMOTE):$(VPS_APP)/"; \
	ssh $(VPS_REMOTE) "$(VPS_APP)/native/apps/desktop/build-vps.sh"

release:
	cd $(DESKTOP_DIR) && node release.mjs

# O Windows compila no Windows e publica daqui: o publish-release.sh é bash e o segredo
# da API mora no WSL. Os dois instaladores vão com chave própria no manifesto — quem
# instalou pelo .msi receberia `InvalidUpdaterFormat` se baixasse o .exe de volta.
#
# O filtro pela versão não é enfeite: a pasta de saída junta build de todas as versões,
# e publicar o .msi de ontem com o número de hoje deixa todo mundo baixando o errado.
publish-windows:
	@set -e; \
	source $(HOME)/auxilos/release-secret.env; \
	export RELEASE_SECRET; \
	cd $(DESKTOP_DIR); \
	for file in $(WINDOWS_APPS)/Unkvoid_$(VERSAO)_*.msi; do \
		./publish-release.sh windows-x86_64-msi "$$file" "$$file.sig"; \
	done; \
	for file in $(WINDOWS_APPS)/Unkvoid_$(VERSAO)_*-setup.exe; do \
		./publish-release.sh windows-x86_64-nsis "$$file" "$$file.sig"; \
	done

install: mac-build
	@set -e; \
	app="$(CURDIR)/native/target/release/bundle/macos/Unkvoid.app"; \
	if pgrep -x unkvoid-desktop >/dev/null 2>&1; then \
		osascript -e 'tell application "Unkvoid" to quit' >/dev/null 2>&1 || true; \
		sleep 2; \
		pkill -x unkvoid-desktop 2>/dev/null || true; \
	fi; \
	rm -rf /Applications/Unkvoid.app; \
	ditto "$$app" /Applications/Unkvoid.app; \
	open -a /Applications/Unkvoid.app
