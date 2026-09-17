<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Resources\Api\VoiceTokenResource;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use JsonException;
use Symfony\Component\HttpKernel\Exception\NotFoundHttpException;

/**
 * A sala por código de quem está logado. Não há o que autorizar: o código é a sala, e quem
 * o tem entra. O token existe para a conta chegar ao SFU, que mantém uma sessão por conta.
 */
final class RoomController
{
    /**
     * Código de 26 caracteres é o ULID de um canal: um token de sala com ele pularia a
     * permissão de conectar.
     *
     * @throws JsonException
     */
    public function token(string $code, #[CurrentUser] User $user, SfuClient $sfu): VoiceTokenResource
    {
        throw_if(mb_strlen($code) === 26, NotFoundHttpException::class);

        return new VoiceTokenResource($sfu->roomToken($code, $user));
    }
}
