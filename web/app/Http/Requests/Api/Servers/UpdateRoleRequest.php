<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use App\Enums\PermissionEnum;
use Illuminate\Foundation\Http\FormRequest;

final class UpdateRoleRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'name' => ['sometimes', 'string', 'max:100'],
            'color' => ['sometimes', 'nullable', 'string', 'regex:/^#[0-9a-fA-F]{6}$/'],
            'permissions' => ['sometimes', 'integer', 'min:0', 'max:'.PermissionEnum::all()],
            'position' => ['sometimes', 'integer', 'min:1', 'max:4294967295'],
        ];
    }
}
