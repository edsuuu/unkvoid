<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Friends;

use Illuminate\Foundation\Http\FormRequest;

final class StoreFriendRequest extends FormRequest
{
    /**
     * @return array<string, array<int, string>>
     */
    public function rules(): array
    {
        return [
            'email' => ['required', 'string', 'email', 'max:255'],
        ];
    }
}
