<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use App\Models\ErrorReport;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/**
 * @mixin ErrorReport
 */
final class ErrorReportResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'signature' => $this->signature,
            'version' => $this->version,
            'platform' => $this->platform,
            'occurrences' => $this->occurrences,
            'first_seen_at' => $this->first_seen_at->toIso8601String(),
            'last_seen_at' => $this->last_seen_at->toIso8601String(),
        ];
    }
}
