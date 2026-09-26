<?php

declare(strict_types=1);

namespace App\Http\Requests\Auth;

use Illuminate\Foundation\Http\FormRequest;

final class AppLoginRequest extends FormRequest
{
    /**
     * @return array<string, array<int, string>>
     */
    public function rules(): array
    {
        return [
            // Até a 0.0.28 o app esperava numa porta local; o de agora volta pelo
            // esquema `unkvoid://` e não manda porta nenhuma.
            'port' => ['sometimes', 'integer', 'min:1024', 'max:65535'],
            'state' => ['required', 'string', 'max:64', 'regex:/^[0-9a-fA-F]+$/'],
            // O app que sabe renovar pede o par, como no login por senha.
            'refresh' => ['sometimes', 'boolean'],
        ];
    }
}
