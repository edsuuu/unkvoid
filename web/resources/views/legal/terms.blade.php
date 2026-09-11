<x-guest-layout :title="__('Termos de uso')">
    <article class="lp-article">
        <h1>Termos de uso</h1>
        <p class="lp-date">Última atualização: 11 de setembro de 2026</p>

        <p>Ao usar o Unkvoid você concorda com o que está aqui. São poucas regras, e todas existem por um motivo.</p>

        <h2>O serviço</h2>
        <p>O Unkvoid é um aplicativo de compartilhamento de tela. Você cria uma sala, recebe um código, e quem tiver o código vê o que você transmitir. A imagem passa pelo nosso servidor apenas para ser retransmitida, e não é gravada.</p>

        <h2>A conta</h2>
        <p>A entrada é feita com uma conta Google. Você é responsável pelo que acontece nas salas criadas com a sua conta. Se achar que alguém está usando a sua conta, troque a senha no Google e avise a gente.</p>

        <h2>O que não pode</h2>
        <ul>
            <li>Transmitir conteúdo ilegal, ou que viole direitos de outra pessoa.</li>
            <li>Usar o serviço para assediar, ameaçar ou expor alguém.</li>
            <li>Tentar entrar em salas adivinhando códigos, ou interferir no funcionamento do serviço.</li>
            <li>Redistribuir o app modificado como se fosse o original.</li>
        </ul>
        <p>Quem faz isso perde a conta, sem aviso prévio quando o caso for grave.</p>

        <h2>O código da sala</h2>
        <p>O código é a chave da sala. Quem tem o código entra. Mande só para quem você quer lá dentro. O dono da sala pode remover pessoas, e a pessoa removida não consegue voltar.</p>

        <h2>Relatórios de erro</h2>
        <p>Quando o app fecha sozinho ou registra um erro, o trecho do log daquele momento é enviado para a gente na próxima vez que ele abre. É assim que descobrimos falhas que só acontecem em alguns computadores, e que ninguém consegue descrever depois.</p>
        <p>Vai junto a versão do app, o sistema operacional e as linhas do log. O nome de usuário do seu computador é trocado por <span class="lp-mono">&lt;usuario&gt;</span> antes do envio. Não vai o conteúdo da sua tela, nem áudio, nem o que foi transmitido: o log registra o que o programa fez, não o que você mostrou.</p>
        <p>O arquivo fica no seu computador também, e o app mostra onde, na janela de diagnóstico. Se preferir que nada seja enviado, é só não instalar o app: assistir pelo navegador não envia relatório nenhum.</p>

        <h2>Ao baixar</h2>
        <p>Baixar ou instalar o Unkvoid é aceitar estes termos e a <a href="{{ route('privacy') }}" wire:navigate>Política de privacidade</a>. Se você não concorda com alguma parte, não instale.</p>

        <h2>Disponibilidade</h2>
        <p>O Unkvoid é um projeto pequeno, oferecido como está. Pode ficar fora do ar para manutenção, e pode ter erros. Fazemos o possível para avisar antes e consertar rápido, mas não garantimos disponibilidade contínua nem nos responsabilizamos por prejuízos decorrentes de indisponibilidade.</p>

        <h2>Encerramento</h2>
        <p>Você pode apagar a sua conta quando quiser, pelo e-mail em <a href="{{ route('privacy') }}" wire:navigate>Privacidade</a>. Nós podemos encerrar o serviço, avisando com antecedência razoável.</p>

        <h2>Mudanças</h2>
        <p>Se estes termos mudarem, a data no topo muda junto. Continuar usando depois da mudança é aceitar a versão nova.</p>

        <h2>Lei aplicável</h2>
        <p>Estes termos seguem a lei brasileira. Dúvidas: <a href="mailto:contato@unkvoid.com">contato@unkvoid.com</a>.</p>
    </article>
</x-guest-layout>
