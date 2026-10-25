<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

final class StoreServerSoundRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            // mp3, ogg e wav: os que o app toca. 512 KB dá alguns segundos de som; o app corta em 5 s.
            'sound' => ['required', 'file', 'mimetypes:audio/mpeg,audio/ogg,audio/wav,audio/x-wav,audio/wave', 'max:512'],
            'name' => ['required', 'string', 'min:1', 'max:32'],
            'emoji' => ['nullable', 'string', 'max:32'],
            'volume' => ['nullable', 'integer', 'min:0', 'max:100'],
        ];
    }
}
