<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

final class StoreInviteRequest extends FormRequest
{
    /**
     * A validade em segundos e o limite de usos são os do Discord; 0 é "nunca" e "sem limite".
     *
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'channel_id' => ['nullable', 'string', 'regex:/^[0-9a-z]{26}$/'],
            'max_age' => ['nullable', 'integer', Rule::in([0, 1800, 3600, 21600, 43200, 86400, 604800])],
            'max_uses' => ['nullable', 'integer', Rule::in([0, 1, 5, 10, 25, 50, 100])],
        ];
    }
}
