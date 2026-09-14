<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\ChannelOverwrite;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin ChannelOverwrite
 */
final class OverwriteResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'target_type' => $this->target_type->value,
            'target_id' => $this->target_id,
            'allow' => $this->allow,
            'deny' => $this->deny,
        ];
    }
}
