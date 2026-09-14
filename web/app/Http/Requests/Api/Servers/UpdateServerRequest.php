<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

final class UpdateServerRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'name' => ['required', 'string', 'max:100'],
        ];
    }
}
