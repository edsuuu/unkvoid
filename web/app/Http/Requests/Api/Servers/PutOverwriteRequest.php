<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use App\Enums\PermissionEnum;
use Illuminate\Foundation\Http\FormRequest;

final class PutOverwriteRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'allow' => ['required', 'integer', 'min:0', 'max:'.PermissionEnum::all()],
            'deny' => ['required', 'integer', 'min:0', 'max:'.PermissionEnum::all()],
        ];
    }
}
