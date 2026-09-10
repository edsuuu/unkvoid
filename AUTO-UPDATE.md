# Atualização automática e releases

Como ligar o auto-update e como sair uma versão para os três sistemas.

O app procura versão nova ao abrir e de seis em seis horas. Se achar, baixa,
instala e reinicia sozinho — a menos que você esteja numa sala, porque reiniciar
no meio de uma transmissão derrubaria quem está assistindo. Nesse caso a versão
fica no disco e passa a valer no próximo reinício.

## O que faz a atualização funcionar

Três coisas, e as três precisam existir ao mesmo tempo:

| Peça | Onde vive |
|---|---|
| Chave pública | `native/apps/desktop/src-tauri/tauri.conf.json`, em `plugins.updater.pubkey` |
| Chave privada | `~/.tauri/unkvoid.key` na máquina que gera o build |
| Manifesto | `latest.json`, publicado junto dos instaladores na release do GitHub |

A pública vai dentro do app. A privada assina cada instalador e produz o arquivo
`.sig` ao lado dele. O manifesto diz, para cada sistema, qual arquivo baixar e
qual é a assinatura dele. **Sem assinatura o instalador sobe igual e ninguém se
atualiza** — a release parece certa e não atualiza ninguém.

A chave privada não tem senha e **não** está no repositório. Ela é o que prova
que a atualização veio de você: quem a tiver publica atualização para todo mundo
que instalou o app. Faça uma cópia em lugar seguro; se ela sumir, a única saída
é gerar outro par e trocar a pública no app, e aí quem já instalou precisa
reinstalar uma vez à mão.

## Preparar, uma vez só

### 1. GitHub

Em **Settings → Secrets and variables → Actions → New repository secret**:

| Nome | Valor |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | o conteúdo inteiro de `~/.tauri/unkvoid.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | vazio |

Para copiar a chave:

```bash
cat ~/.tauri/unkvoid.key | pbcopy
```

### 2. VPS

O Linux é compilado lá, e quem publica a release é o `gh`. Ele precisa estar
autenticado uma vez:

```bash
ssh vps
gh auth login
```

O resto — Rust, cmake, dependências do Tauri — o `build-vps.sh` instala sozinho
na primeira execução.

## Soltar uma versão

### 1. Suba o número

Em `native/apps/desktop/src-tauri/tauri.conf.json`, campo `version`.

Suba junto o `SFU_APP_VERSION` em `sfu/ecosystem.config.cjs` e o padrão em
`sfu/src/config.ts`, senão o app vê a versão do servidor diferente da sua e
procura atualização a cada abertura, para sempre.

```bash
cd sfu && ./deploy.sh
```

### 2. Publique a tag

```bash
git commit -am "Sobe para 0.0.4"
git tag v0.0.4
git push origin main --tags
```

A tag dispara o `.github/workflows/release.yml`, que compila o macOS e depois o
Windows em runners de verdade. Os dois trabalhos são **encadeados de propósito**:
cada um publica lendo o `latest.json` que já está lá e mesclando o próprio alvo
por cima. Em paralelo, o segundo apagaria a plataforma do primeiro.

### 3. Gere o Linux

```bash
make build-vps
```

Ele compila na VPS, publica na mesma release e refaz o repositório APT. O `.dmg` e
o `.msi` não saem de lá: um exige um Mac por licença da Apple, o outro exige o WiX
rodando no Windows.

### 4. Confira

```bash
gh release view v0.0.4 --repo edsuuu/unkvoid
curl -sL https://github.com/edsuuu/unkvoid/releases/latest/download/latest.json | jq '.platforms | keys'
```

Devem aparecer estas chaves:

```
darwin-aarch64
linux-x86_64
linux-x86_64-appimage
linux-x86_64-deb
windows-x86_64
windows-x86_64-msi
windows-x86_64-nsis
```

O sufixo do instalador não é enfeite. O app procura
`{sistema}-{arquitetura}-{instalador}` e só depois a chave genérica. Enquanto
`.deb` e AppImage dividiam a chave `linux-x86_64`, quem instalou pelo `.deb`
baixava o AppImage, a verificação de formato falhava e a atualização morria em
silêncio.

## Linux: quem atualiza é o APT

No Linux o app **não** se atualiza sozinho. Quem cuida disso é o gerenciador de
pacotes, que é o que quem usa Linux espera, e manter os dois caminhos ligados
faria o app pedir senha de root no meio da abertura para fazer o que o
`apt upgrade` já faz junto com o resto do sistema.

O repositório vive na VPS, em `/var/www/apt`, e é servido em
<https://discord.unkvoid.com/apt/>. O `build-vps.sh` refaz o índice a cada build.

### Instalar, uma vez por máquina

```bash
curl -fsSL https://discord.unkvoid.com/apt/unkvoid.gpg \
  | sudo tee /etc/apt/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/etc/apt/keyrings/unkvoid.gpg] https://discord.unkvoid.com/apt ./" \
  | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

Depois disso, versão nova entra com `sudo apt upgrade`.

### A chave do repositório

É **outra** chave, diferente da do auto-update. A do auto-update assina o
instalador; esta assina a lista de pacotes, e é o que impede alguém no meio do
caminho oferecer um `.deb` trocado.

| | |
|---|---|
| Identidade | `repo@unkvoid.com` |
| Onde | chaveiro do usuário `ubuntu` na VPS |
| Pública publicada em | `/var/www/apt/unkvoid.gpg` |

Ela não tem senha, porque o build assina sem ninguém por perto. Guarde uma cópia:

```bash
ssh vps "gpg --export-secret-keys --armor repo@unkvoid.com" > unkvoid-apt.key
```

Se ela sumir, gere outra e todo mundo precisa refazer o passo do `curl` acima.

## Gerar um instalador sem publicar

Para testar sem mexer na release:

```bash
ssh vps "UNKVOID_BUNDLES=deb TAURI_SIGNING_PRIVATE_KEY=\$(cat) \
  /var/www/projects/unkvoid/native/apps/desktop/build-vps.sh --dry-run" < ~/.tauri/unkvoid.key
```

O resultado fica em <https://discord.unkvoid.com/downloads/>.

A chave viaja pela entrada padrão de propósito: assim ela não aparece na linha de
comando, que qualquer um enxerga com `ps`, e não fica gravada em disco na VPS.

## Quando não atualiza

| Sintoma | Causa provável |
|---|---|
| A release existe e ninguém atualiza | Build sem `TAURI_SIGNING_PRIVATE_KEY`: não há `.sig`, e o `latest.json` sai sem a plataforma |
| Só uma plataforma atualiza | Os dois trabalhos rodaram em paralelo e um sobrescreveu o manifesto do outro |
| Nada acontece, sem erro | A release foi marcada como pré-lançamento. O app procura em `/releases/latest/`, e o "latest" do GitHub ignora pré-lançamento |
| Erro de formato no Linux | O `.deb` recebeu a URL do AppImage: confira os sufixos no `latest.json` |

Para ver o que o app achou, botão **Logs** dentro do app, depois **Copiar logs**.


## O manifesto no nosso próprio servidor

O atualizador embutido consulta dois endereços, em ordem:

1. `https://discord.unkvoid.com/downloads/latest.json`
2. a release do GitHub, como reserva

O primeiro existe para o auto-update não depender do GitHub Actions. Cada
sistema é compilado numa máquina diferente — Windows no Windows, macOS num Mac,
Linux na VPS — e as três chamam o mesmo script:

```
native/apps/desktop/publish-downloads.sh 0.0.7 windows-x86_64 caminho/do/Unkvoid.msi
```

Ele copia o instalador e a assinatura para a pasta que o nginx serve, e
**costura** a entrada da plataforma no `latest.json` em vez de reescrevê-lo:
publicar o Windows não pode apagar o macOS que subiu ontem. A versão do
manifesto é sempre a mais nova que já passou por ali, então uma correção só para
um sistema não rebaixa o que os outros anunciam.

O build da VPS chama o script sozinho, para o AppImage. O `.deb` fica de fora de
propósito: quem instala por pacote atualiza pelo repositório APT, com
`apt upgrade`, e não pelo atualizador embutido.

Sem o arquivo `.sig` ao lado, o script recusa. Um manifesto apontando para um
instalador não assinado é uma atualização que ninguém consegue instalar.
