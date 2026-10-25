# Clips

Clips é o replay instantâneo local do aplicativo **para Windows**. Ele não depende de conta, servidor ou internet: pode continuar gravando mesmo sem conexão com o Unkvoid.

## Como funciona

Com o replay ligado, o aplicativo mantém um buffer de tela e áudio em disco. Ao salvar, transforma o trecho recente em um arquivo MP4 sem codificar o H.264 de novo. A captura e o encoder usam o caminho da placa de vídeo e são separados da transmissão ao vivo; gravar e transmitir ao mesmo tempo abre os dois caminhos.

Os Clips ficam na pasta escolhida pela pessoa. A galeria local permite abrir e apagar os arquivos. As preferências existentes incluem ligar ou desligar o replay, duração do trecho, qualidade, taxa de quadros, pasta de destino e atalhos para abrir o painel ou salvar o replay.

## Limites atuais

- O suporte é exclusivo do Windows; os outros sistemas não têm captura e encoder de Clips.
- Não há upload, sincronização, compartilhamento por servidor, rota REST nem dado de Clip no Laravel.

Os atalhos globais só são registrados depois que o replay foi ligado ao menos uma vez. Se outro programa já usar a combinação escolhida, o Windows pode deixar a tecla chegar aos dois programas.
