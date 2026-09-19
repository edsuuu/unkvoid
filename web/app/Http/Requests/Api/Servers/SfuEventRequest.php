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
            'event' => ['required', Rule::in(['joined', 'left'])],
            // O canal é um ULID de 26 letras; a sala por código tem de 3 a 32.
            'room' => ['required', 'string', 'regex:/^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$|^[0-9a-z]{26}$/i'],
            'sub' => ['required', 'string', 'regex:/^user:\d+$|^guest:[A-Za-z0-9-]{1,64}$/'],
            'name' => ['required', 'string', 'max:255'],
            'ip' => ['required', 'string', 'max:45'],
            'at' => ['required', 'integer'],
        ];
    }
}
