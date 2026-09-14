<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Clips;

use App\Models\Clip;
use Illuminate\Http\Response;

/**
 * Sem Sanctum: o player de vídeo não manda cabeçalho. O que protege é a assinatura da URL,
 * que só sai no `ClipResource` de quem clipou.
 */
final class PlaylistClipController
{
    public function __invoke(string $clip): Response
    {
        $playlist = Clip::findReady($clip)->playlist();

        abort_if(is_null($playlist), 404);

        return response($playlist, 200, ['Content-Type' => 'application/vnd.apple.mpegurl']);
    }
}
