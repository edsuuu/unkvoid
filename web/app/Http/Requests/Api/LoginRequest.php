<?php

declare(strict_types=1);

namespace App\Http\Requests\Api;

use Illuminate\Foundation\Http\FormRequest;

final class LoginRequest extends FormRequest
{
    /**
     * @return array<string, array<int, string>>
     */
    public function rules(): array
    {
        return [
            'email' => ['required', 'email'],
            'password' => ['required', 'string'],
            'device' => ['required', 'string', 'max:60'],
        ];
    }

    public function email(): string
    {
        return mb_strtolower(trim((string) $this->input('email')));
    }

    public function password(): string
    {
        return (string) $this->input('password');
    }

    /** Nome do aparelho: aparece na lista de sessões e permite revogar uma só. */
    public function device(): string
    {
        return (string) $this->input('device');
    }
}
