@props(['title'])
<!DOCTYPE html>
<html lang="pt-BR">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="color-scheme" content="dark">
    <title>{{ $title }}</title>
</head>
<body style="margin:0;padding:0;background:#06050a;color:#ece9f3;font-family:Archivo,Helvetica,Arial,sans-serif;-webkit-font-smoothing:antialiased">
    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:#06050a">
        <tr>
            <td align="center" style="padding:40px 16px">
                <table role="presentation" width="560" cellpadding="0" cellspacing="0" border="0" style="max-width:560px;width:100%">
                    <tr>
                        <td align="center" style="padding:0 0 22px">
                            <a href="{{ config('app.url') }}" style="text-decoration:none;color:#ece9f3">
                                <img src="{{ asset('images/unkvoid-mark.png') }}" alt="Unkvoid" width="34" height="34" style="display:inline-block;vertical-align:middle;border:0">
                                <span style="display:inline-block;vertical-align:middle;margin-left:10px;font-size:16px;font-weight:600;letter-spacing:-0.01em">Unkvoid</span>
                            </a>
                        </td>
                    </tr>
                    <tr>
                        <td style="background:#12101c;border:1px solid #241f34;border-radius:22px;padding:34px 30px">
                            {{ $slot }}
                        </td>
                    </tr>
                    <tr>
                        <td align="center" style="padding:22px 10px 0;font-family:'IBM Plex Mono',Menlo,Consolas,monospace;font-size:11.5px;line-height:1.7;color:#6f6889">
                            Este e-mail foi enviado por <a href="mailto:contato@unkvoid.com" style="color:#9a9cff;text-decoration:none">contato@unkvoid.com</a>.<br>
                            <a href="{{ route('privacy') }}" style="color:#8a80a6;text-decoration:none">Privacidade</a>
                            &nbsp;·&nbsp;
                            <a href="{{ route('terms') }}" style="color:#8a80a6;text-decoration:none">Termos de uso</a>
                        </td>
                    </tr>
                </table>
            </td>
        </tr>
    </table>
</body>
</html>
