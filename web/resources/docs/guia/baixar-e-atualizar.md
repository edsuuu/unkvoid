# Baixar e atualizar

Os links de download ficam na página inicial do Unkvoid. Ela mostra somente as plataformas para as quais há uma versão publicada no momento.

## Pacotes

No Windows, o Unkvoid está disponível pela Microsoft Store e pelo instalador `.exe`. O serviço também pode publicar instaladores para macOS (`.dmg`) e Linux (`.deb`). O pacote do Linux também pode ser distribuído pelo repositório APT do Unkvoid.

Um link de uma plataforma sem versão publicada retorna 404; isso evita oferecer um arquivo que não existe.

## Atualizações

O aplicativo consulta `GET /downloads/latest.json` para descobrir a versão mais recente compatível. O manifesto fornece a URL temporária do arquivo e a assinatura da versão.

- No Windows e no macOS, o aplicativo confere a assinatura Minisign antes de oferecer a atualização.
- No Linux, o repositório APT usa uma chave GPG; as atualizações são feitas pelo `apt`.
- O `.dmg` do macOS é para instalação inicial. O mecanismo de atualização usa o artefato próprio do aplicativo quando ele estiver publicado.

O manifesto não fica em cache para que uma atualização recém-publicada apareça sem esperar uma cópia antiga expirar.

> Baixe somente pelos links do Unkvoid. A confirmação de assinatura serve justamente para que o aplicativo recuse um pacote alterado.
