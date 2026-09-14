<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use App\Enums\PermissionEnum;
use Illuminate\Foundation\Http\FormRequest;

final class StoreRoleRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'name' => ['required', 'string', 'max:100'],
            'color' => ['nullable', 'string', 'regex:/^#[0-9a-fA-F]{6}$/'],
            'permissions' => ['required', 'integer', 'min:0', 'max:'.PermissionEnum::all()],
        ];
    }
}
