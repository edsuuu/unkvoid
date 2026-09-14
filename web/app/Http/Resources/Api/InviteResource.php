<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

final class InviteResource extends JsonResource
{
    public function __construct(private readonly string $inviteCode)
    {
        parent::__construct($inviteCode);
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return ['invite_code' => $this->inviteCode];
    }
}
