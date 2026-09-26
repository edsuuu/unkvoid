<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Auth;

use Illuminate\Foundation\Http\FormRequest;

final class LoginRequest extends FormRequest
{
    /**
     * @return array<string, array<int, string>>
     */
    public function rules(): array
    {
        return [
            'email' => ['required', 'string', 'email', 'max:255'],
            'password' => ['required', 'string'],
            'device' => ['required', 'string', 'max:60'],
            // O app que sabe renovar pede o par de tokens; o antigo não manda nada e fica
            // com o token que não vence.
            'refresh' => ['sometimes', 'boolean'],
        ];
    }
}
