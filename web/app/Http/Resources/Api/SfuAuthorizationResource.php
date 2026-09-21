<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * A única resposta da API que não vai embrulhada em `data`: quem lê é o SFU, que espera
 * `{allowed, name}` cru.
 */
final class SfuAuthorizationResource extends JsonResource
{
    public static $wrap;

    public function __construct(private readonly bool $allowed, private readonly string $name)
    {
        parent::__construct(null);
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return ['allowed' => $this->allowed, 'name' => $this->name];
    }
}
