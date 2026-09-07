<?php

declare(strict_types=1);

namespace App\Http\Requests\Api;

use Illuminate\Foundation\Http\FormRequest;

final class SendMessageRequest extends FormRequest
{
    /**
     * @return array<string, array<int, string>>
     */
    public function rules(): array
    {
        return ['content' => ['required', 'string', 'max:2000']];
    }

    public function content(): string
    {
        return trim((string) $this->input('content'));
    }
}
