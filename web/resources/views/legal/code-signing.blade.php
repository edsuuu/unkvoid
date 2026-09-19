<x-guest-layout :title="__('Code signing policy')">
    <article class="lp-article">
        <h1>Code signing policy</h1>

        <p>Free code signing provided by <a href="https://about.signpath.io">SignPath.io</a>, certificate by <a href="https://signpath.org">SignPath Foundation</a>.</p>

        <p>Esta é a política de assinatura de código do Unkvoid: de onde saem os instaladores, quem mexe no código e quem decide se uma versão pode ser assinada.</p>

        <h2>Origem do código e dos instaladores</h2>
        <p>O Unkvoid é um projeto de código aberto, sob a licença MIT. O código-fonte está em <a href="https://github.com/edsuuu/unkvoid">github.com/edsuuu/unkvoid</a>.</p>
        <p>Os instaladores saem de build automatizado do próprio repositório, pelo GitHub Actions, a partir do código publicado lá. Toda versão passa por aprovação manual antes de ser assinada.</p>

        <h2>Papéis do time</h2>
        <p>Hoje o projeto é mantido por uma pessoa só, que ocupa os dois papéis.</p>

        <h3>Committers and reviewers</h3>
        <p>Quem escreve o código e revisa o que entra no repositório.</p>
        <ul>
            <li><a href="https://github.com/edsuuu">edsuuu</a></li>
        </ul>

        <h3>Approvers</h3>
        <p>Quem decide se uma versão pode ser assinada.</p>
        <ul>
            <li><a href="https://github.com/edsuuu">edsuuu</a></li>
        </ul>

        <h2>Privacy policy</h2>
        <p>O app procura atualização sozinho e, quando registra um erro, manda o trecho do log na próxima vez que abre. O que é coletado, por quê e por quanto tempo está na <a href="{{ route('privacy') }}" wire:navigate>Política de privacidade</a>.</p>
    </article>
</x-guest-layout>
