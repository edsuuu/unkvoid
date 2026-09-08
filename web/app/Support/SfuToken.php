<?php

declare(strict_types=1);

namespace App\Support;

use App\Models\Channel;
use App\Models\ServerMember;

final class SfuToken
{
    private const TTL_SECONDS = 21600;

    public function issue(ServerMember $member, Channel $channel): string
    {
        return $this->sign([
            'sub' => $member->user_id,
            'name' => $member->nickname ?? $member->user->displayName(),
            'avatar' => $member->user->avatar_url,
            'room' => $channel->id,
            'server' => $channel->server_id,
            'role' => $member->role,
            'iat' => time(),
            'exp' => time() + self::TTL_SECONDS,
        ]);
    }

    /**
     * Sala avulsa, sem conta por trás.
     *
     * O `sub` é sorteado a cada entrada porque não existe usuário para identificar — e
     * é ele que o SFU usa como id do participante. Fixá-lo pelo nome faria duas pessoas
     * chamadas "Edsu" derrubarem uma à outra da sala.
     *
     * `server` recebe o próprio código: é por ele que a presença agrupa os canais, e
     * aqui a sala e o "servidor" são a mesma coisa.
     */
    public function issueGuest(string $room, string $name): string
    {
        return $this->sign([
            'sub' => 'guest-'.bin2hex(random_bytes(8)),
            'name' => $name,
            'avatar' => null,
            'room' => $room,
            'server' => $room,
            'role' => 'member',
            'iat' => time(),
            'exp' => time() + self::TTL_SECONDS,
        ]);
    }

    /**
     * Presence-only token: lets you track who is in the voice channels of the
     * server without joining any of them.
     */
    public function issuePresence(ServerMember $member): string
    {
        return $this->sign([
            'sub' => $member->user_id,
            'name' => $member->nickname ?? $member->user->displayName(),
            'server' => $member->server_id,
            'role' => $member->role,
            'iat' => time(),
            'exp' => time() + self::TTL_SECONDS,
        ]);
    }

    /**
     * @param  array<string, mixed>  $payload
     */
    private function sign(array $payload): string
    {
        $header = $this->encode(json_encode(['alg' => 'HS256', 'typ' => 'JWT'], JSON_THROW_ON_ERROR));
        $body = $this->encode(json_encode($payload, JSON_THROW_ON_ERROR));
        $signature = hash_hmac('sha256', "{$header}.{$body}", (string) config('services.sfu.secret'), true);

        return "{$header}.{$body}.{$this->encode($signature)}";
    }

    private function encode(string $value): string
    {
        return rtrim(strtr(base64_encode($value), '+/', '-_'), '=');
    }
}
