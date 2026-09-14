<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Facades\Config;

/**
 * O que o app precisa saber antes de logar: onde está o SFU e onde está o Reverb.
 */
final class ConfigResource extends JsonResource
{
    public function __construct()
    {
        parent::__construct(null);
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'sfu' => Config::string('services.sfu.public_url'),
            'reverb' => [
                'host' => Config::string('broadcasting.connections.reverb.options.host'),
                'port' => Config::integer('broadcasting.connections.reverb.options.port'),
                'key' => Config::string('broadcasting.connections.reverb.key'),
                'scheme' => Config::string('broadcasting.connections.reverb.options.scheme'),
            ],
        ];
    }
}
