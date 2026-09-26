<?php

declare(strict_types=1);

namespace App\Http\Requests\Api;

use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

final class StoreErrorReportRequest extends FormRequest
{
    /**
     * O mesmo que o `std::env::consts::OS` do Rust devolve nos três sistemas.
     */
    private const array PLATFORMS = ['windows', 'macos', 'linux'];

    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'version' => ['required', 'string', 'regex:/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/'],
            'platform' => ['required', 'string', Rule::in(self::PLATFORMS)],
            'log' => ['required', 'string', 'max:20000'],
        ];
    }
}
