<?php

declare(strict_types=1);

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\Config;
use Symfony\Component\HttpFoundation\Response;

/**
 * O CI publica instaladores sem conta: assina hora, método, caminho e o SHA-256 do
 * arquivo com o RELEASE_SECRET. Sem segredo configurado, nega tudo.
 */
final class VerifyReleaseSignature
{
    private const int WINDOW_SECONDS = 300;

    /**
     * @param  Closure(Request): Response  $next
     */
    public function handle(Request $request, Closure $next): Response
    {
        $secret = Config::string('unkvoid.release_secret');
        $timestamp = (string) $request->header('X-Unkvoid-Timestamp', '');
        $signature = (string) $request->header('X-Unkvoid-Signature', '');
        $file = $request->file('file');

        abort_if($secret === '' || $timestamp === '' || $signature === '' || ! $file instanceof UploadedFile, 401, 'assinatura ausente');

        abort_if(abs(time() - (int) $timestamp) > self::WINDOW_SECONDS, 401, 'assinatura fora da janela de tempo');

        $expected = hash_hmac('sha256', implode("\n", [$timestamp, $request->method(), '/'.$request->path(), hash_file('sha256', $file->getRealPath())]), $secret);

        abort_unless(hash_equals($expected, $signature), 401, 'assinatura inválida');

        return $next($request);
    }
}
