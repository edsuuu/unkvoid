<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use App\Enums\ChannelTypeEnum;
use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

final class StoreChannelRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'name' => ['required', 'string', 'max:100'],
            'type' => ['required', Rule::enum(ChannelTypeEnum::class)],
            'topic' => ['nullable', 'string', 'max:1024'],
            'user_limit' => ['nullable', 'integer', 'min:1', 'max:99'],
        ];
    }
}
