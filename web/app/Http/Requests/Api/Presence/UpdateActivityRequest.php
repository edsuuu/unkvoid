<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Presence;

use Illuminate\Foundation\Http\FormRequest;

final class UpdateActivityRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'activity' => ['present', 'nullable', 'array'],
            'activity.name' => ['required_with:activity', 'string', 'min:1', 'max:80'],
            'activity.started_at' => ['required_with:activity', 'date'],
        ];
    }
}
