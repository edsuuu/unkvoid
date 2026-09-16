<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

final class UpdateChannelRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'name' => ['sometimes', 'string', 'max:100'],
            'topic' => ['sometimes', 'nullable', 'string', 'max:1024'],
            'position' => ['sometimes', 'integer', 'min:0', 'max:4294967295'],
            'user_limit' => ['sometimes', 'nullable', 'integer', 'min:1', 'max:99'],
        ];
    }
}
