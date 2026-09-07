# Unkvoid — estado do projeto

Documento para quem pegar o trabalho daqui. Diz o que existe, o que **não** existe,
e onde estão as armadilhas que já custaram tempo.

Para **o que cada peça faz e por quê**, veja [ARQUITETURA.md](ARQUITETURA.md).

**Web em produção:** https://discord.unkvoid.com · **Repo:** github.com/edsuuu/unkvoid
**VPS:** 144.126.133.10 (Contabo, St. Louis/EUA)

> **Nome.** O projeto se chama **Unkvoid** desde a v0.6.0 (app, repositório e bundle
> `com.unkvoid.desktop`). O subdomínio segue `discord.unkvoid.com` por enquanto — é a
> única string "discord" que ainda importa. O deep link continua `discord2://` de
> propósito: renomear o esquema exigiria um deploy do web no mesmo instante, e ele não
> aparece para o usuário.

---

## As três partes

```
web/      Laravel 13 + Livewire 4   auth, servidores, canais, chat, API do desktop
sfu/      Node + TypeScript          mídia (mediasoup) e sinalização (WebSocket)
native/   Rust + Tauri               app desktop com captura nativa
```

O `web/` e o `sfu/` estão **em produção e funcionando**. O `native/` está no começo.

---

## O que funciona hoje

### Web (produção)
Login por e-mail/senha e Google, servidores com convite, canais de texto e voz,
chat, voz com compartilhamento de tela pelo SFU, presença por WebSocket, moderação
em três níveis, reconexão que sobrevive a queda de rede e a deploy do SFU.

### Chat em tempo real (produção)
Laravel Reverb sob pm2 na porta **8081** (a 8080 já é do filebrowser desta VPS), atrás do
nginx em `/app`. Mensagem de outra pessoa aparece sem F5.

Não havia poll para eliminar — **havia nada**: quem estava com o canal aberto só via a
mensagem trocando de canal ou recarregando.

### SFU (produção, 4 workers)
API em TypeScript no padrão do MoneyClips: rota → Request → controller → Service →
Resource. Cobre retomada de sessão, presença, moderação e **relay de sinalização P2P**
(`signal`), que existe mas ainda não tem cliente usando.

```bash
cd sfu && pnpm run check   # asserções sobre o contrato inteiro
```

### API para o desktop
`web/routes/api.php`, autenticada por Sanctum com token. Login, servidores, canais,
mensagens, e os mesmos emissores de token do SFU que o web usa.
Testes em `web/tests/Feature/Api/DesktopApiTest.php`.

### App desktop (v0.8.0)
Abre, verifica atualização, exige servidor, pede login (e-mail/senha **ou Google**),
lista servidores e canais com o design do web, **captura a tela nativamente** e
**transmite por P2P**.

**Topologia:** quem envia usa Rust (captura nativa + encoder por hardware, sem barra
do navegador); quem recebe usa o WebRTC do próprio webview e um `<video>`. A
limitação do WKWebView era só o `getDisplayMedia` — receber vídeo ele faz bem, e
assim não é preciso decodificar nem desenhar em Rust.

**Áudio do sistema entra na transmissão** em Opus (48 kHz estéreo, blocos de 20 ms),
como segunda trilha da mesma conexão.

> **O som do próprio app fica de fora.** Quem filtra é o macOS, por processo
> (`with_excludes_current_process_audio`), não um `if` no nosso código. Sem isso,
> compartilhar áudio devolveria a voz de quem está na chamada e criaria
> realimentação. A crate documenta essa opção literalmente como *"Prevent feedback"*.
>
> Filtrar por processo é mais confiável que tentar adivinhar a origem do som: o
> sistema sabe exatamente o que saiu de qual aplicativo.

**Um encoder, N conexões:** o quadro é comprimido uma vez e enviado a cada
espectador. Codificar por espectador derreteria a máquina de quem transmite — o
custo do P2P é banda de upload, não CPU.

A sinalização vai pelo WebSocket do SFU (ação `signal`): mesma sala, mesma
autenticação, nenhum canal novo. O app não abre socket próprio — duas sessões com o
mesmo id de participante fazem o SFU derrubar uma delas.

---

## Quantas pessoas assistindo

| Espectadores | Caminho | Custo para quem transmite |
|---|---|---|
| 1–3 | direto, máquina a máquina | ~20 ms de latência, upload × N |
| 4+ | pelo SFU (`producePlain`) | ~139 ms (VPS nos EUA), upload constante |

A troca é automática: quem entra como quarto espectador dispara `subirParaOSfu()`, as
conexões diretas são fechadas e a transmissão passa a subir **uma vez** para o
servidor. Subir para os dois ao mesmo tempo anularia o ganho.

**Como o app fala com o SFU sem WebRTC.** É RTP puro sobre UDP no `PlainTransport` do
mediasoup: sem ICE, sem DTLS. O lado Rust escolhe SSRC, payload type e a chave SRTP e
anuncia tudo em `producePlain` **antes** do primeiro pacote; `comedia` faz o servidor
aprender o endereço de origem do primeiro pacote que chega, então o app não precisa
ser alcançável de fora. `crates/media/src/plain.rs`.

> **SRTP não é opcional aqui.** Sem ele a tela atravessaria a internet em claro. A
> chave é gerada por transmissão, vive no processo e só sai dentro do WebSocket
> autenticado.

Isso também fechou um buraco que não era só de escala: **quem usa o web nunca
conseguiu ver uma transmissão vinda do app**, porque o app só falava P2P. Agora o
produtor é igual a qualquer outro da sala.

⚠️ **Portas:** `41000-41031/udp` precisam estar liberadas no firewall (8 por worker,
4 workers). Sem elas o caminho acima de 3 espectadores não recebe nada. As de sempre
(`40000-40003/udp`) continuam valendo para o WebRTC normal.

**Verificação real, não no papel:**

```bash
cargo run -p media --example plain -- ws://127.0.0.1:3000/sfu <token>
```

Entra numa sala, declara a transmissão, manda H.264 sintético e **espera o servidor
confirmar que está recebendo** (evento `producerActive`). Isso só acontece se SSRC,
payload type e chave SRTP baterem — um pacote que sai não é um pacote que foi
entendido.

---

## Chat de voz

O microfone existe nos dois (web e app) e usa o **mesmo código**: o app importa
`SfuClient` e `MicrophoneGate` de `web/resources/js/voice/` via alias do Vite. Duas
cópias de um protocolo de reconexão divergem, e a que diverge é sempre a que ninguém
está olhando.

No app a voz vai pelo **SFU** (que replica para quantas pessoas forem) enquanto a tela
fica direta — áudio é barato, vídeo não.

`MicrophoneGate` decide quadro a quadro se o áudio sai:

| Modo | Comportamento |
|---|---|
| Detecção de voz (padrão) | abre acima do limiar, com janela de 300 ms para não picotar palavra |
| Apertar para falar | só com a tecla segurada (`event.code`, funciona em qualquer layout) |

Mute é o gate, **nunca** o producer: abrir e fechar producer a cada sílaba renegocia o
transporte dezenas de vezes por minuto, e refazer `getUserMedia` pisca o indicador de
microfone do sistema.

```bash
node web/resources/js/voice/MicrophoneGate.check.mjs
```

> **Krisp não dá.** É SDK proprietário licenciado comercialmente pelo Discord, sem
> distribuição pública. A supressão em uso é a nativa do Chrome/WKWebView
> (`noiseSuppression: true`). Se não bastar, o upgrade real é RNNoise (open source),
> WASM no web e nativo no Rust.

---

## Builds: os três existem (v0.8.0)

`.msi`, `.deb` e `.dmg` são publicados pelo CI a cada tag `v*`, junto com o
`latest.json` que o auto-update procura. Instalador **não pode ser cross-compilado**
(`.msi` exige Windows, `.deb` exige Linux) — por isso o workflow tem três runners.

https://github.com/edsuuu/unkvoid/releases

### O que foi verificado por plataforma

| Peça | macOS | Windows | Linux |
|---|---|---|---|
| `capture` | roda | compila no runner | compila no runner |
| `media` (encoder + WebRTC) | roda e medido | compila, **nunca executado** | compila, **nunca executado** |
| App Tauri | roda | **não executado** | **não executado** |
| Instalador | `.dmg` publicado | `.msi` publicado | `.deb` publicado |
| RTP puro para o SFU | verificado ponta a ponta | mesmo código, não executado | idem |

### O que o app faz hoje fora do macOS

Abre, entra em sala e **fala** — o microfone é do webview, funciona nos três. O que
não funciona é **transmitir a tela**: o encoder por hardware só existe no macOS
(VideoToolbox). Fora dele `PlatformEncoder::new` devolve `EncoderError::Unsupported`
e a interface mostra o erro.

O stub existe com a **mesma forma** do encoder real de propósito — sem isso o app nem
compilaria fora do mac, e aí nem o `.msi` sairia.

### Para destravar

1. Encoder no Windows: Media Foundation, espelhando `crates/media/src/macos.rs`.
2. Captura no Linux: consumir o nó do PipeWire que o portal XDG devolve.

---

## O que NÃO existe

| Peça | Situação |
|---|---|
| Encoder no Windows | falta Media Foundation — sem ele o app não transmite lá |
| Captura no Linux | recusa com erro claro; falta consumir o nó do PipeWire |
| Áudio de sistema no Windows | precisa de WASAPI loopback, separado do Graphics Capture |
| Trocar qualidade sem parar | o app web faz; no desktop exige reiniciar a transmissão |
| Chat no desktop | lista mensagens em texto cru, sem enviar |
| SFU em Rust (str0m) | não começou |

**A captura de tela nunca rodou nesta máquina**: a permissão de gravação foi negada
aqui. O encoder, a negociação WebRTC e o RTP para o SFU **foram testados** — o que
falta é a ponta a ponta com tela real e duas pessoas.

---

## Próximo passo sugerido

Nesta ordem — cada etapa é verificável sozinha:

1. **Encoder no Windows** (Media Foundation). Sem ele o app abre e fala no Windows,
   mas não transmite a tela — e é justamente lá que está quem joga. Espelhar
   `crates/media/src/macos.rs`, que já tem a forma certa.
2. **Captura no Linux** (PipeWire, a partir do nó que o portal XDG devolve).
3. **Chat no desktop**: a API já envia e lê; falta a interface.
4. **Trocar qualidade sem parar a transmissão** no desktop — o web já faz.

### Perfis de qualidade

Escolhidos na interface e válidos ponta a ponta — a mesma opção define a resolução da
captura **e** o bitrate do encoder:

| Perfil | Resolução | Bitrate | Custo medido |
|---|---|---|---|
| 720p | 1280×720 | 4 Mbps | — |
| 1080p | 1920×1080 | 7 Mbps | 7,86 ms/quadro (47%) |
| 1440p | 2560×1440 | 12 Mbps | 12,25 ms/quadro (73%) |

Áudio: Opus a 96 kbps, 48 kHz estéreo, independente do perfil de vídeo.

### Verificações que existem

```bash
# Rust
cargo test --workspace                 # blocos Opus de 20 ms, pacotização e SRTP em socket real
cargo run -p media --example encoder   # encoder por hardware, ms/quadro
cargo run -p media --example peer      # oferta com H.264 + ICE
cargo run -p media --example p2p       # negociação completa entre dois lados
cargo run -p media --example plain -- ws://127.0.0.1:3000/sfu <token>
                                       # o SFU CONFIRMANDO que recebe o RTP puro
cargo run -p capture --example spike   # captura (exige permissão de tela)

# Web e SFU
cd web  && php artisan test            # 43 testes, com a autorização do broadcast
cd web  && node resources/js/voice/MicrophoneGate.check.mjs
cd sfu  && pnpm run check              # o contrato inteiro, com o RTP puro
cd native/apps/desktop && npm run check # nenhum id ou classe apontando para o vazio
```

### Números já medidos do encoder

| Resolução | ms/quadro | Orçamento a 60 fps | Uso |
|---|---|---|---|
| 1080p | 7,86 ms | 16,67 ms | 47% |
| 1440p | 12,25 ms | 16,67 ms | 73% |

Medido com superfície estática — tela real dá mais trabalho. 1440p60 é o limite.

Por que P2P primeiro: a VPS está nos EUA e os usuários no Brasil — **139 ms de RTT
medidos**. Dois brasileiros direto ficam em ~20 ms.

---

## Armadilhas já pagas

**Microfone**
- **O gate não pode medir a própria saída.** Fechar o portão com `track.enabled = false`
  zera o medidor junto, o nível nunca mais sobe acima do limiar e o microfone fica mudo
  o resto da chamada. O que é publicado é um **clone**; o medidor escuta o original.
  `MicrophoneGate.check.mjs` tem uma asserção só para isso.
- **Medidor em `setTimeout` não serve.** Em janela em segundo plano o navegador
  estrangula o timer: 60 ms medidos viraram **1000 ms**. Um AudioWorklet entrega a cada
  53 ms na mesma janela oculta. Sem isso, minimizar o app corta um segundo do início de
  cada frase.
- Apertar-para-falar no navegador **só funciona com a janela em foco** — não existe
  atalho global. E `blur` tem que soltar o gate: sem isso, largar a tecla fora da janela
  deixa o microfone aberto para sempre.

**Tauri**
- **`withGlobalTauri` é obrigatório aqui.** A UI (`native/apps/desktop/ui/`) não passa
  por bundler, então só alcança o Rust por `window.__TAURI__`. Sem essa flag no
  `tauri.conf.json` o objeto não existe, a primeira linha do `app.js` estoura, e o app
  fica **parado para sempre na tela de atualização** — sem erro visível, porque quem
  mostraria o erro é justamente o script que morreu. Foi assim da v0.2.0 à v0.5.0.
  Os plugins (`opener`, `deepLink`) só se registram *depois* que esse objeto existe:
  o `api-iife.js` de cada um começa com `if ("__TAURI__" in window)`.
- `app.js` agora checa `window.__TAURI__?.core` e escreve o motivo na própria tela.
  Uma tela travada não diz nada; uma tela com o motivo diz tudo.
- `updater.check()` sem `timeout` fica pendurado no timeout do sistema quando o
  endpoint aceita a conexão e não responde. São 10 s em `check_update`.
- Identificador terminado em `.app` conflita com a extensão de bundle do macOS —
  daí `com.unkvoid.desktop` e não `com.unkvoid.app`.

**Auto-update**
- ⚠️ **Não marque a release como pré-lançamento.** O endpoint é
  `/releases/latest/download/latest.json`, e o "latest" do GitHub **ignora**
  pré-lançamentos: com todas marcadas assim, essa URL responde **404** e o app nunca acha
  atualização nenhuma. Foi assim da v0.2.0 à v0.7.0 — o auto-update existia e nunca
  rodou uma vez.
- A versão do `latest.json` sai do `tauri.conf.json`, não da tag. Se divergirem o cliente
  entra em laço: instala, continua anunciando a versão antiga, e atualiza de novo.

**Google no app**
- As rotas do OAuth do desktop viviam em `routes/api.php`, **que não tem sessão**, e o
  Socialite guarda o `state` do OAuth nela: **500 "Session store not set on request"**.
  Agora elas usam o grupo `web`. `stateless()` seria trocar o erro por um buraco de CSRF.
- ⚠️ **`https://discord.unkvoid.com/api/desktop/google/callback` precisa estar nos URIs
  de redirecionamento autorizados** do cliente OAuth no Google Cloud. É um caminho
  diferente do web, e sem ele o Google recusa com `redirect_uri_mismatch`.
- Um 302 para `discord2://` é **bloqueado sem aviso** por vários navegadores: eles
  impedem redirecionamento automático para esquema externo. O callback devolve uma
  **página** com o link clicável — é o clique que faz o navegador perguntar "abrir o
  Unkvoid?", que é a permissão de que o fluxo depende.

**Windows**
- Testar o servidor com uma rota **autenticada** quebra de um jeito difícil de enxergar:
  sem token ela responde 302 para `/login`, o fetch segue o redirecionamento, e a página
  de login não tem cabeçalho CORS — o fetch rejeita e o app conclui que o servidor caiu.
  Era exatamente a tela "No connection" num app recém-instalado. Por isso existe
  `/api/health`, pública.
- O Windows bloqueia conexão de **entrada** por padrão, e o WebRTC precisa receber para
  o ICE fechar: sem regra de firewall a transmissão direta entre duas máquinas não
  conecta. Sair não precisa de permissão, então o caminho pelo SFU funciona de qualquer
  jeito. A regra entra pelo instalador (`src-tauri/wix/windows.wxs`) com
  `Return="ignore"` — instalação que quebra por causa de rede seria pior.
- O template WiX do Tauri **já cria o atalho da área de trabalho**
  (`ApplicationDesktopShortcut`, dentro de `ShortcutsFeature`). Declarar `DesktopFolder`
  outra vez num fragmento duplica o símbolo e o `light` falha **sem imprimir o erro** —
  foi o que derrubou a v0.8.0. Para saber o que o template já tem, inspecione um MSI
  pronto: `msiinfo export <app>.msi Directory`.

**Reverb**
- `ShouldBroadcast` **enfileira**. Sem worker (esta VPS não tem), a mensagem salva e
  simplesmente não chega. É `ShouldBroadcastNow`, e tem teste só para isso.
- O Livewire fixa os listeners no **mount**. Um canal escolhido depois nunca se inscreve
  — por isso a inscrição está em `ChatSocket.js`, não num listener Echo do componente.
- A configuração vai no **HTML**, não no bundle: os assets são compilados na máquina de
  quem desenvolve, então `VITE_REVERB_HOST` viraria "localhost" em produção.
- São **dois endereços** para o mesmo serviço: Laravel → Reverb usa a porta interna;
  navegador → Reverb passa pelo nginx em 443/wss. Confundir os dois é o erro que faz o
  chat tentar conectar em localhost.
- `NullBroadcaster` autoriza **qualquer** canal. Um teste de permissão contra ele passa
  sem provar nada — daí `BROADCAST_CONNECTION=reverb` no `phpunit.xml`.

**Renomear**
- HTML, CSS e JS da UI do desktop compartilham ids e classes. Renomear só o JS quebra a
  tela em silêncio: o navegador não reclama de classe que não existe. `npm run check`
  em `native/apps/desktop` confere que todo id procurado existe.
- Um id pode ter hífen; um nome de variável não. `trilha` era os dois, e virou
  `const server-rail = …`, que não compila.
- Detectar comentário por `^\s*\*` confunde `*ativo` (deref em Rust) com a continuação
  de um `/** */`.

**CI**
- O `GITHUB_TOKEN` deste repo é read-only por padrão: publicar release volta **403
  "Resource not accessible by integration"**. Resolve com `permissions: contents: write`
  no workflow.
- `cargo test` **compila os examples**. `encoder.rs` linka IOSurface e derrubava a
  verificação em Linux e Windows. Gatear o example com `cfg(target_os)` conserta clippy,
  test e qualquer `--all-targets` de uma vez — melhor que remendar cada comando do CI.
- `if: env.X == ''` num *step* **não enxerga** o `env:` daquele mesmo step, e o contexto
  `secrets` não existe em `if` de step. A secret tem que virar `env` no nível do **job**.
  Enquanto isso estava errado, a condição era sempre verdadeira e **todo instalador saía
  com o updater desligado**.
- O build de Windows morria em `icons/icon.ico not found`. `npx tauri icon icons/icon.png`
  gera o set inteiro (o `.ico` e o `.icns` entram no repo; as pastas `android/` e `ios/`
  que ele cria são lixo aqui).
- Os `examples/` da crate `media` linkam IOSurface: só passam clippy no macOS. Fora dele
  o CI roda `--lib --bins --tests`.
- `createUpdaterArtifacts` **não** gera o `latest.json` — ele só assina os bundles. O job
  `manifest` monta o manifesto a partir dos `.sig` e publica na release; sem ele o
  auto-update procura um arquivo que não existe e toma 404.

**Build**
- O Opus vem da crate `opus`, que compila libopus do zero via **cmake**. Os runners do
  GitHub já têm cmake; localmente foi preciso `brew install cmake`.
- Uma tradução pt→en aplicada sem fronteira de palavra passou por cima de
  `sfu/node_modules` (`echo`→`andcho`, `command`→`withmand`). 156 arquivos. Nada
  versionado foi afetado; `rm -rf node_modules && pnpm install` resolveu.

**Rust / macOS**
- A crate `screencapturekit` compila Swift e o linker procura o runtime no caminho do
  Xcode completo. Com só os Command Line Tools o caminho é outro — resolvido em
  `native/.cargo/config.toml`, junto do alvo mínimo 13.0.
- Sem `NSScreenCaptureUsageDescription` no Info.plist o macOS nem mostra o pedido de
  permissão. Está em `native/apps/desktop/src-tauri/Info.plist`.
- O app não é assinado nem notarizado: o Gatekeeper bloqueia. `xattr -dr
  com.apple.quarantine` resolve. Assinar exige conta Apple paga.

**SFU**
- `pm2 restart <nome> --update-env` relê o ambiente do **shell**, não o
  `ecosystem.config.cjs`. Mudança no ecosystem exige `pm2 delete` + `pm2 start`.
- `pnpm install` não baixa o worker do mediasoup e **sai com erro** por isso. O deploy
  roda o postinstall na mão e valida o binário.
- O mediasoup é **ICE Lite**: só responde, nunca inicia. Atrás de firewall stateful a
  porta de mídia precisa aceitar entrada não solicitada. São 4 portas (40000-40003),
  uma por worker — `WebRtcServer` não é compartilhável entre processos.

**Laravel**
- Usuários são **UUID**. A tabela do Sanctum precisou de `uuidMorphs`, não `morphs`.
- `#[Override]` em `rules()` de FormRequest quebra: o pai não declara o método.
- `wire:ignore` no palco de voz é obrigatório — sem ele o Livewire recria o trecho a
  cada render e leva os `<video>` junto.

**Rede**
- O firewall da Contabo tem allowlist por porta. Abertas: 22, 80, 443, 8443,
  30033/tcp, 9987/udp e 40000-40003 (tcp+udp).
- ⚠️ **Falta abrir `41000-41031/udp`** — é por onde o app desktop entrega a
  transmissão ao SFU acima de 3 espectadores. Sem isso os pacotes saem e não chegam,
  e a transmissão fica preta para quem assiste (o caminho direto, até 3, não usa
  essas portas e continua funcionando).

---

## Deploy

```bash
./infra/deploy-web.sh    # Laravel
./sfu/deploy.sh          # SFU
```

`.env` e `storage` vivem em `shared/` e sobrevivem ao deploy. Nenhum segredo está no
repositório.

App desktop: tag `v*` dispara o CI, que gera `.msi`, `.deb` e `.dmg` em runners
nativos e anexa na Release. Instalador não pode ser cross-compilado.

**A chave privada do updater não está no repositório.** Ela precisa ir para os
secrets do GitHub como `TAURI_SIGNING_PRIVATE_KEY` para o CI assinar as atualizações.
Se ela for perdida, nenhuma versão futura consegue atualizar as já instaladas.

---

## Decisões que valem entender antes de mudar

- **Sem Redis.** Cache, fila e sessão em banco.
- **Sem Reverb.** Chat por `wire:poll`; presença por WebSocket do próprio SFU.
- **Sem navegação de página no workspace.** Trocar de canal é estado Livewire — é o
  que impede a chamada de cair. A URL é reescrita com `history.replaceState`.
- **Tema escuro fixo.** O alternador do template causava texto escuro sobre fundo
  escuro quando o SO estava em claro.
- **`maintain-framerate` + `contentHint: motion`** e piso de 30 fps na captura. Com
  `maintain-resolution` o FPS oscilava entre 5 e 60.
