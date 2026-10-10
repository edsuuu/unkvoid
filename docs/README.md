# Documentação

Um arquivo por pergunta. Chegando agora: [ARQUITETURA.md](ARQUITETURA.md). Quer saber o que
falta: [ESTADO.md](ESTADO.md).

## Entender o projeto

| Quero… | Vá em |
|---|---|
| o mapa: cada peça, como conversam, os fluxos, onde roda | [ARQUITETURA.md](ARQUITETURA.md) |
| o app nativo: as camadas, o que é de cada sistema, a ponte do Swift | [APP-NATIVO.md](APP-NATIVO.md) |
| uma rota, um evento, o formato do token, uma ação do SFU | [CONTRATO.md](CONTRATO.md) |
| por que algo foi feito assim (ex.: por que o SFU é Node) | [DECISOES.md](DECISOES.md) |
| o que falta, o que nunca rodou em hardware, o que espera o dono | [ESTADO.md](ESTADO.md) |
| o que é cifrado, o que está protegido e o que não está | [SEGURANCA.md](SEGURANCA.md) |
| por que o Windows 10 mostrava uma borda amarela, e como ela saiu | [BORDA-AMARELA.md](BORDA-AMARELA.md) |

## Rede e servidor

| Quero… | Vá em |
|---|---|
| o caminho da imagem e cada ajuste de rede medido | [REDE.md](REDE.md) |
| quais portas UDP abrir, e o que quebra calado quando falta | [UDP.md](UDP.md) |
| a VPS de hoje: medições, firewall, APT, o que desligar | [SERVIDOR.md](SERVIDOR.md) |
| levantar uma VPS do zero, em ordem, e migrar o e-mail | [INSTALAR-VPS.md](INSTALAR-VPS.md) |

## Buildar e publicar

| Quero… | Vá em |
|---|---|
| o instalador do Windows e o pacote da Microsoft Store | [BUILD-WINDOWS.md](BUILD-WINDOWS.md) |
| o `.deb` do Linux e o laboratório em contêiner | [BUILD-LINUX.md](BUILD-LINUX.md) |
| o `.app` do macOS | [BUILD-MACOS.md](BUILD-MACOS.md) |
| assinar, publicar uma versão e descobrir por que alguém não atualizou | [AUTO-UPDATE.md](AUTO-UPDATE.md) |

## Fora desta pasta

| Arquivo | Para quê |
|---|---|
| [../README.md](../README.md) | o que é o projeto, instalar, o caminho curto para rodar |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | ambiente, rodar local, o que verificar antes do PR, regras de código |
| [../CLAUDE.md](../CLAUDE.md) | as mesmas regras, na forma que os agentes de código leem |
| `../native/apps/*/README.md` | cada interface: rodar, onde vai cada coisa, o que é daquele sistema |
| `../sfu/README.md` | o SFU: pastas, variáveis de ambiente, rodar e testar |
| `../native/tests/linux/README.md` | os cenários do Linux no contêiner |
| `../web/tests/checklist.html`, `../native/apps/desktop/checklist.html` | o que já foi validado à mão |
