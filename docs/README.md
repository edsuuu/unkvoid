# Documentação

Um arquivo por pergunta. Comece pela [ARQUITETURA.md](ARQUITETURA.md) se está chegando agora,
e pelo [ESTADO.md](ESTADO.md) se quer saber o que falta.

## Entender o projeto

| Quero… | Vá em |
|---|---|
| o mapa: cada peça, como conversam, os fluxos, onde roda | [ARQUITETURA.md](ARQUITETURA.md) |
| as interfaces nativas: o desenho, o que já é nativo, por onde começar | [APP-NATIVO.md](APP-NATIVO.md) |
| ver uma rota, um evento, o formato do token, um comando do Tauri | [CONTRATO.md](CONTRATO.md) |
| entender por que algo foi feito assim (ex.: por que o SFU é Node) | [DECISOES.md](DECISOES.md) |
| saber o que falta, o que nunca rodou em hardware e o que espera o dono | [ESTADO.md](ESTADO.md) |
| saber o que é cifrado, o que está protegido e o que não está | [SEGURANCA.md](SEGURANCA.md) |
| saber por que o Windows 10 mostra uma borda amarela ao transmitir, e o plano para tirá-la | [BORDA-AMARELA.md](BORDA-AMARELA.md) |

## Rede e servidor

| Quero… | Vá em |
|---|---|
| o caminho da imagem e cada ajuste de rede medido | [REDE.md](REDE.md) |
| quais portas UDP abrir, e o que quebra calado quando falta | [UDP.md](UDP.md) |
| a VPS que existe hoje: medições, firewall, APT, o que desligar | [SERVIDOR.md](SERVIDOR.md) |
| levantar uma VPS do zero, em ordem, e migrar o e-mail | [INSTALAR-VPS.md](INSTALAR-VPS.md) |

## Buildar e publicar

| Quero… | Vá em |
|---|---|
| gerar o instalador do Windows (a partir do WSL, inclusive) | [BUILD-WINDOWS.md](BUILD-WINDOWS.md) |
| gerar o `.app` e o `.dmg` | [BUILD-MACOS.md](BUILD-MACOS.md) |
| compilar e testar o Linux num contêiner | [BUILD-LINUX.md](BUILD-LINUX.md) |
| assinar, publicar uma versão e descobrir por que alguém não atualizou | [AUTO-UPDATE.md](AUTO-UPDATE.md) |

## Fora desta pasta

| Arquivo | Para quê |
|---|---|
| [../README.md](../README.md) | o que é o projeto, instalar, o caminho curto para rodar |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | ambiente, rodar local, o que verificar antes do PR, regras de código, armadilhas já pagas |
| [../CLAUDE.md](../CLAUDE.md) | as mesmas regras, na forma que os agentes de código leem |
| `../native/tests/linux/README.md` | os cenários do Linux no contêiner |
| `../web/tests/checklist.html`, `../native/apps/desktop/checklist.html` | o que já foi validado à mão, no site e no app |
