<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Auth;

use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

final class UpdateMeRequest extends FormRequest
{
    /**
     * @return array<string, string>
     */
    public function messages(): array
    {
        return [
            'name.regex' => 'O apelido aceita letras, números, ponto e _ — sem espaço.',
            'name.unique' => 'Esse apelido já é de outra pessoa.',
            'banner_color.regex' => 'A cor da faixa vai no formato #rrggbb.',
        ];
    }

    /**
     * Ignorar a própria conta é o que deixa ficar com o apelido automático.
     *
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'name' => ['sometimes', 'required', 'string', 'min:3', 'max:32', 'regex:/^[A-Za-z0-9._]+$/', Rule::unique('users', 'name')->ignore($this->user()?->id)],
            'bio' => ['sometimes', 'nullable', 'string', 'max:190'],
            'pronouns' => ['sometimes', 'nullable', 'string', 'max:40'],
            'banner_color' => ['sometimes', 'nullable', 'string', 'regex:/^#[0-9a-fA-F]{6}$/'],
            'custom_status' => ['sometimes', 'nullable', 'string', 'max:128'],
            'status' => ['sometimes', 'required', 'string', Rule::in(['online', 'idle', 'dnd', 'invisible'])],
            'status_minutes' => ['sometimes', 'nullable', 'integer', Rule::in([15, 60, 480, 1440, 4320])],
        ];
    }
}
