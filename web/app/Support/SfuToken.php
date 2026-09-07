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
     * Token só de presença: deixa acompanhar quem está nos canais de voz do
     * servidor sem entrar em nenhum deles.
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
