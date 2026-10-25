<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Auth;

use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rules\Password;

final class StartRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            // O nome vira o apelido (`User::freeNickname`): acento e espaço saem lá.
            'name' => ['required', 'string', 'max:32'],
            'password' => ['required', 'string', Password::min(8)],
            'device' => ['required', 'string', 'max:60'],
            'refresh' => ['sometimes', 'boolean'],
        ];
    }
}
