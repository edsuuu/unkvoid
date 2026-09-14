<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Services\Sfu\SfuClient;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Facades\Config;

final class VoiceTokenResource extends JsonResource
{
    public function __construct(private readonly string $token)
    {
        parent::__construct($token);
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'token' => $this->token,
            'url' => Config::string('services.sfu.public_url'),
            'expires_in' => SfuClient::TOKEN_SECONDS,
        ];
    }
}
