<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Auth;

use Illuminate\Foundation\Http\FormRequest;

final class ExchangeAppCodeRequest extends FormRequest
{
    /**
     * @return array<string, array<int, string>>
     */
    public function rules(): array
    {
        return [
            'code' => ['required', 'string', 'min:32', 'max:128', 'regex:/^[A-Za-z0-9_-]+$/'],
            'code_verifier' => ['required', 'string', 'min:43', 'max:128', 'regex:/^[A-Za-z0-9._~-]+$/'],
        ];
    }
}
