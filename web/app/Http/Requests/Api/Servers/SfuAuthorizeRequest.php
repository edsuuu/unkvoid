<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

/**
 * Canal desconhecido não vira 422: quem decide é o `User::canSubscribe()`, e o que ele não
 * conhece vira `allowed: false`.
 */
final class SfuAuthorizeRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'sub' => ['required', 'string', 'max:64'],
            'channel' => ['required', 'string', 'max:64'],
            'at' => ['required', 'integer'],
        ];
    }
}
