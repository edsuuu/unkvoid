<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

final class UpdateMemberRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'nickname' => ['sometimes', 'nullable', 'string', 'max:32'],
            'role_ids' => ['sometimes', 'array'],
            'role_ids.*' => ['integer'],
            'server_mute' => ['sometimes', 'boolean'],
            'server_deaf' => ['sometimes', 'boolean'],
        ];
    }
}
