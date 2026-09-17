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
            // O canal é um ULID de 26 letras; a sala por código tem de 3 a 32, e só o
            // visitante (`guest:`) entra nela.
            'room' => ['required_if:event,joined,left', 'string', 'regex:/^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$|^[0-9a-z]{26}$/i'],
            'sub' => ['required_if:event,joined,left', 'string', 'regex:/^user:\d+$|^guest:[A-Za-z0-9-]{1,64}$/'],
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
