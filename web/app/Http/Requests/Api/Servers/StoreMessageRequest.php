<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

final class StoreMessageRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'body' => ['required', 'string', 'min:1', 'max:2000'],
            'reply_to_id' => ['nullable', 'integer'],
        ];
    }
}
