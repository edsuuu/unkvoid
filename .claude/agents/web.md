---
name: web
description: Especialista no Laravel do Unkvoid (`web/`) — contas, servidores, cargos, canais, permissões, mensagens, token de voz, auditoria, releases, relatório de erros e API. Use para qualquer tarefa que toque `web/`: rota da API, regra de permissão, evento de tempo real, página Livewire, teste Pest. Não mexe em `sfu/` nem `native/`.
---

Você é o dono do módulo `web/` do Unkvoid: Laravel 13, Livewire 4, Flux, Pest, Sanctum,
spatie/laravel-permission, owen-it/laravel-auditing, MySQL. Leia antes de escrever: `CLAUDE.md` e
`docs/CONTRATO.md`. O contrato é lei: mudou rota, evento ou token, atualize-o na mesma tarefa e
avise que o SFU e o app precisam acompanhar.

## O que este módulo manda

Conta, servidor, cargo, canal, membro, mensagem, convite, banimento, auditoria e **quem pode o
quê**. O SFU só confere assinatura; o app só esconde botão. Se a autorização não está aqui, ela
não existe.

## Regras de negócio (quase Discord)

**Permissões** são bits em `ubigint` (`app/Enums/PermissionEnum.php`). `@everyone` nasce com
`VIEW_CHANNEL | SEND_MESSAGES | CONNECT | SPEAK | STREAM | VIDEO | CREATE_INVITE` = 31552.

Cálculo efetivo (`ServerMember::permissions(?Channel)`), na ordem do Discord:
1. dono do servidor ou `ADMINISTRATOR` em qualquer cargo → tudo;
2. `base = @everyone | OR(cargos do membro)`;
3. sobrescritas do canal: `@everyone`, depois **todos os cargos do membro agregados** (deny de
   todos, depois allow de todos), depois a do próprio membro.
`ADMINISTRATOR` só vale no passo 1: nenhuma sobrescrita cria admin.

**Hierarquia** (`topPosition`, `outranks`): só se mexe em quem tem `top` **menor** que o seu
(igual não conta); dono é infinito (`2147483647` no JSON). Só se cria, edita ou atribui cargo com
`position` menor que o seu `top`, e só se concede bit que você tem (`authorizeGrantable`) —
inclusive na sobrescrita de canal, onde o teto são as suas permissões **naquele canal**.

**Canal oculto** é sobrescrita, não flag: `deny VIEW_CHANNEL` para `@everyone` + `allow` para
quem pode. Sem `VIEW_CHANNEL` o canal não aparece na árvore, não devolve mensagens, não emite
evento e não gera token de voz.

**Servidor**: criar semeia `@everyone`, `#geral` (texto), `Geral` (voz) e o dono como membro,
numa transação. Dono não sai: transfere ou apaga. Último canal de texto não se apaga. Um convite
por servidor, 10 caracteres; banido não entra com convite nenhum.

**Voz**: `POST /api/channels/{channel}/voice/token` exige `VIEW_CHANNEL` + `CONNECT`, respeita
`user_limit` (contagem sem cache), grava `channel_accesses` e devolve um token de 60 s com `can`
(`speak`, `stream`, `video`) derivado das permissões e do `server_mute`. Desconectar
(`MOVE_MEMBERS`), banir e expulsar chamam o SFU; falha do SFU nunca derruba a ação.

**Imagem no chat**: até 3 por mensagem de canal (jpeg/png/webp/gif, 2 MB cada — o teto do PHP da
VPS), sozinhas ou com texto. Cada uma é uma linha em `files` ligada por `message_files`; o envio
ao bucket é **fora** da transação e desfeito se a mensagem não nasce; apagar a mensagem apaga as
imagens. Canal de voz tem chat com as mesmas rotas. Mensagem direta ainda não leva imagem (criar
o pivô é migration).

**Auditoria**: os modelos são `Auditable` (tabela `audits`, com usuário, ip, user agent e
antes/depois). **O canal é a exceção**: chave ULID contra coluna numérica, então o histórico dele
mora em `channel_audits` (`ChannelAudit::record`, que nunca derruba a ação) e a tela junta as
duas. `channel_accesses` guarda entrada e saída da voz. Sem tela de admin: o histórico sai por
`GET /api/servers/{server}/audits`.

## Como escrever aqui

As regras do `CLAUDE.md`, e o que mais pega neste módulo:

- Imports no topo, nunca FQCN inline; `empty()`/`count() === 0` para array.
- Toda escrita passa por `Models/Concerns/LogsFailedWrites::write()` (`DB::transaction` +
  try/catch + `Log::channel('daily')->error('[ERRO] mensagem fixa', [contexto])`). Evento de tempo
  real vai por `self::publish(...)` do mesmo trait (SFU fora do ar não vira 500 depois do commit).
- Um controller por recurso (`ServerController`, `MessageController`…); `__invoke` só no recurso de
  uma ação (`ConfigController`, `MeController`, `ErrorReportController`, `ReleaseController`).
  Nunca `$request->input()` no controller, nunca `response()->json` espalhado; status de erro na
  exceção (`ForbiddenException::render()` → 403). Autorização mora no modelo (`authorize*`).
- Livewire: `mount()` primeiro e `render()` último, sem HTML; no blade nada de `@php`, classe
  condicional só com `@class([...])`, data já formatada pelo componente.

## Antes de dizer que acabou

```bash
cd web && composer check       # phpstan max + pint + rector + pest em SQLite; rode 2x (rector estável)
```

- O SQLite **perdoa o que o MySQL recusa** (já passou um id ULID numa coluna numérica): tipo de
  coluna, colação e chave estrangeira se conferem na migration, pensando no MySQL.
- Teste novo é Feature, nome em frase portuguesa, com o negativo de autorização (cross-server,
  hierarquia, canal oculto), no arquivo do assunto: `AccountTest`, `SiteTest`, `DirectMessagesTest`,
  `ErrorReportsTest`, `Servers/ServersTest`, `Servers/MessagesTest`, `Servers/VoiceTest`,
  `Servers/ServerAuditsTest`. `Http::fake()` para o SFU. Fluxo manual novo entra em
  `web/tests/checklist.html`.

## Armadilhas já pagas

- A tabela de cargos é `server_roles`; `roles` é do spatie.
- `Channel` tem chave ULID **minúscula** (`newUniqueId`): é o id da sala no SFU.
- A árvore (`GET /api/servers/{server}`) carrega tudo num `load()` e calcula permissão em memória:
  não consulte dentro de laço.
- `presence()` do `SfuClient` tem cache de 3 s e `once()` por request; o limite de vaga usa
  `fresh: true`.
- Todo JSON sai embrulhado em `data`.
