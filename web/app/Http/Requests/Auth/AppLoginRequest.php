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
            // O app de Tauri volta pelo esquema `unkvoid://` e não manda porta; o nativo, e
            // todo pedido com PKCE, espera na porta local.
            'port' => ['required_with:code_challenge', 'integer', 'min:1024', 'max:65535'],
            'state' => ['required', 'string', 'max:64', 'regex:/^[0-9a-fA-F]+$/'],
            // O app que sabe renovar pede o par, como no login por senha.
            'refresh' => ['sometimes', 'boolean'],
            'code_challenge' => ['sometimes', 'required', 'string', 'size:43', 'regex:/^[A-Za-z0-9_-]+$/'],
            'code_challenge_method' => ['required_with:code_challenge', 'in:S256'],
        ];
    }
}
