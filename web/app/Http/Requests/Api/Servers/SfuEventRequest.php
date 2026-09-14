<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

final class SfuEventRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'event' => ['required', Rule::in(['joined', 'left', 'clip.ready', 'clip.failed'])],
            'room' => ['required_if:event,joined,left', 'string', 'size:26'],
            'sub' => ['required_if:event,joined,left', 'string', 'regex:/^user:\d+$/'],
            'name' => ['required_if:event,joined,left', 'string', 'max:255'],
            'ip' => ['required_if:event,joined,left', 'string', 'max:45'],
            'clipId' => ['required_if:event,clip.ready,clip.failed', 'string', 'size:26', 'alpha_num:ascii'],
            'durationMs' => ['required_if:event,clip.ready', 'integer', 'min:0'],
            'sizeBytes' => ['required_if:event,clip.ready', 'integer', 'min:0'],
            'reason' => ['nullable', 'string', 'max:1000'],
            'at' => ['required', 'integer'],
        ];
    }
}
