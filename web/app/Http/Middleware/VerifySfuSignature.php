<?php

declare(strict_types=1);

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Config;
use Symfony\Component\HttpFoundation\Response;

/**
 * O SFU avisa quem entrou e saiu da voz assinando hora, método, caminho e corpo com o
 * SFU_SECRET, o mesmo que assina o token de entrada. Sem segredo configurado, nega tudo;
 * a mesma assinatura só vale uma vez dentro da janela.
 *
 * `signed.sfu:repeatable` desliga só essa última parte, para rota que não muda nada. A
 * mesma conta com o app aberto em duas máquinas se inscreve no mesmo canal no mesmo
 * segundo: mesmo corpo, mesma hora, mesma assinatura — e a segunda seria recusada,
 * deixando um dos dois sem chat por um motivo que não é de segurança.
 */
final class VerifySfuSignature
{
    private const int WINDOW_SECONDS = 300;

    /**
     * @param  Closure(Request): Response  $next
     */
    public function handle(Request $request, Closure $next, ?string $mode = null): Response
    {
        $secret = Config::string('services.sfu.secret');
        $timestamp = (string) $request->header('X-Unkvoid-Timestamp', '');
        $signature = (string) $request->header('X-Unkvoid-Signature', '');

        abort_if($secret === '' || $timestamp === '' || $signature === '', 401, 'assinatura ausente');

        abort_if(abs(time() - (int) $timestamp) > self::WINDOW_SECONDS, 401, 'assinatura fora da janela de tempo');

        $expected = hash_hmac('sha256', implode("\n", [$timestamp, $request->method(), '/'.$request->path(), $request->getContent()]), $secret);

        abort_unless(hash_equals($expected, $signature), 401, 'assinatura inválida');

        if ($mode !== 'repeatable') {
            abort_unless(Cache::add('sfu:signature:'.$signature, true, self::WINDOW_SECONDS), 401, 'assinatura repetida');
        }

        return $next($request);
    }
}
