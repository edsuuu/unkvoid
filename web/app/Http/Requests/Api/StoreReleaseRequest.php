<?php

declare(strict_types=1);

namespace App\Http\Requests\Api;

use App\Enums\ReleasePlatformEnum;
use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

final class StoreReleaseRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'version' => ['required', 'string', 'regex:/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/'],
            'platform' => ['required', Rule::enum(ReleasePlatformEnum::class)],
            'file' => ['required', 'file', 'max:204800'],
            'signature' => ['nullable', 'string', 'max:2000'],
            'notes' => ['nullable', 'string', 'max:2000'],
        ];
    }
}
