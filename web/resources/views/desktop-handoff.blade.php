{{--
    Devolve a autenticação ao app.

    Um 302 direto para `discord2://` é bloqueado sem aviso por vários navegadores:
    redirecionamento automático para esquema externo é justamente o que eles impedem.
    Quem tem que disparar o link é um clique da pessoa — e o navegador então pergunta
    "abrir o Unkvoid?", que é a permissão de que o fluxo depende.

    A tentativa automática fica: onde funciona, ninguém clica em nada. Onde não funciona,
    o botão está na tela em vez de uma página em branco.
--}}
<!doctype html>
<html lang="pt-BR">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{ $erro ? __('Sign-in failed') : __('Signed in') }} · Unkvoid</title>
    <style>
        body {
            display: flex; align-items: center; justify-content: center;
            min-height: 100vh; margin: 0;
            background: #1e1f22; color: #dbdee1;
            font: 15px/1.5 system-ui, -apple-system, sans-serif;
        }
        .card { max-width: 380px; padding: 32px; text-align: center; }
        h1 { margin: 0 0 8px; font-size: 20px; color: #fff; }
        p { margin: 0 0 24px; color: #b5bac1; }
        a.button {
            display: inline-block; padding: 12px 24px; border-radius: 6px;
            background: #5865f2; color: #fff; font-weight: 500; text-decoration: none;
        }
        a.button:hover { background: #4752c4; }
        .hint { margin-top: 20px; font-size: 13px; color: #949ba4; }
        .error { color: #f23f43; }
    </style>
</head>
<body>
    <div class="card">
        @if ($erro)
            <h1 class="error">{{ __('Sign-in failed') }}</h1>
            <p>{{ $erro }}</p>
            <a class="button" href="{{ $link }}">{{ __('Back to Unkvoid') }}</a>
        @else
            <h1>{{ __('You are signed in') }}</h1>
            <p>{{ __('Now go back to the app to finish.') }}</p>
            <a class="button" href="{{ $link }}" id="open">{{ __('Open Unkvoid') }}</a>
            <p class="hint">{{ __('If the browser asks for permission to open Unkvoid, allow it — that is how the app receives your sign-in.') }}</p>
        @endif
    </div>

    <script>
        // Só depois de pintar: a tentativa automática não pode acontecer antes de o
        // botão existir, senão quem for bloqueado fica olhando para uma página vazia.
        addEventListener('load', () => setTimeout(() => { location.href = @json($link); }, 400));
    </script>
</body>
</html>
