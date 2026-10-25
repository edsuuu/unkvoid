# API para integrações

A API JSON atual é a que o próprio aplicativo usa. Ela não possui chaves de desenvolvedor, OAuth para terceiros ou garantia de compatibilidade além do contrato versionado no repositório. Trate-a como integração com uma conta do Unkvoid, usando `Authorization: Bearer` de um token do Sanctum, e valide erros `403` e `422`.

## Endpoints públicos

| Rota | Finalidade |
| --- | --- |
| `GET /api/config` | Descobre a URL pública do SFU. |
| `POST /api/auth/register` | Cria conta com e-mail, senha e identificação do dispositivo. |
| `POST /api/auth/login` | Entra e recebe token de acesso; pode receber token de renovação. |
| `POST /api/auth/refresh` | Troca um token de renovação por um par novo. |
| `POST /api/errors` | Envia relatório técnico limitado por IP. |
| `GET /downloads/latest.json` | Lê o manifesto de atualização. |
| `GET /downloads/{plataforma}` | Redireciona para o pacote publicado da plataforma. |

## Endpoints com conta

Depois de autenticar, a API expõe estes grupos:

- `GET`, `PATCH` e rotas de avatar em `/api/me` para perfil.
- `GET` e `POST /api/servers`, mais leitura, edição, saída, ícone, convite e auditoria em `/api/servers/{server}`.
- Entrada por `POST /api/invites/{code}`.
- Cargos em `/api/servers/{server}/roles` e `/api/roles/{role}`.
- Canais, sobrescritas, mensagens e leitura em `/api/channels/{channel}/...` e `/api/messages/{message}`.
- Token, movimento e desconexão de voz em `/api/channels/{channel}/voice/...`.
- Amigos em `/api/friends/...` e mensagens diretas em `/api/dm/...`.

As rotas respeitam a mesma autorização do aplicativo: ter um token não substitui ser membro, ver o canal ou possuir a permissão necessária.

## O que não é público para integração

`/api/sfu/events`, `/api/sfu/authorize`, `/api/releases` e as rotas HTTP internas do SFU usam assinaturas entre serviços. Não as chame de um cliente externo e não há segredo a copiar desta documentação. A mídia também não é enviada pela API REST: a conta pede um token de voz e o aplicativo fala com o SFU pelo protocolo de sinalização.

Para os campos, respostas e regras completos, consulte `docs/CONTRATO.md` na mesma versão do código com a qual a integração será usada.
