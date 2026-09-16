<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Friends;

use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

final class UpdateFriendRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'action' => ['required', 'string', Rule::in(['accept', 'block'])],
        ];
    }
}
