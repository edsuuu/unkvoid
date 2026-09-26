<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\User;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

final class AuthTokenResource extends JsonResource
{
    /**
     * @param  array{token: string, refresh_token: string|null, expires_at: string|null}  $tokens
     */
    public function __construct(
        private readonly User $user,
        private readonly array $tokens,
    ) {
        parent::__construct($user);
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'token' => $this->tokens['token'],
            'refresh_token' => $this->tokens['refresh_token'],
            'expires_at' => $this->tokens['expires_at'],
            'user' => new UserResource($this->user),
        ];
    }
}
