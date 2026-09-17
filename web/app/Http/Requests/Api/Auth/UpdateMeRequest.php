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
            'name' => ['required', 'string', 'min:3', 'max:32', 'regex:/^[A-Za-z0-9._]+$/', Rule::unique('users', 'name')->ignore($this->user()?->id)],
        ];
    }
}
