# Configurações e arquivos

## Conta e servidor

A conta pode alterar o nome e enviar ou remover uma foto de perfil. Quem possui `MANAGE_SERVER` pode alterar o nome do servidor e enviar ou remover seu ícone. Imagens de perfil e ícones aceitam JPEG, PNG ou WebP de até 2 MB.

Um canal pode receber nome, tópico, posição, categoria pai e, no caso de voz, limite de pessoas. Cargos têm nome, cor, posição e o conjunto de permissões. As regras específicas de um canal são sobrescritas para cargo ou membro.

## Arquivos

Fotos de perfil, ícones de servidor e imagens de chat ficam em um bucket privado. A API devolve URLs pré-assinadas, com prazo de duas horas, para que o arquivo não se torne público apenas por conhecer o caminho.

Não há suporte ativo para anexar imagens a mensagens diretas. Os [Clips](/documentacao/guia/clips) do Windows são arquivos locais: não sobem para este bucket, não passam pela API e não pertencem ao servidor. Criar anexos para mensagem direta ou Clips sincronizados exigiria modelo de dados e contrato novos.

## O que não é uma configuração pública

Os endereços do SFU, segredos de assinatura e controles operacionais são configuração da infraestrutura, não opções de usuário nem dados para integrar de fora. Para descobrir o endpoint de sinalização do aplicativo, a única configuração pública é `GET /api/config`.
