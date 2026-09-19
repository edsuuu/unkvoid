<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

final class StoreMessageRequest extends FormRequest
{
    /**
     * O teto de 3 × 2 MB acompanha o PHP da VPS: 2 MB por arquivo e 8 MB por pedido.
     *
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'body' => ['required_without:images', 'nullable', 'string', 'max:2000'],
            'reply_to_id' => ['nullable', 'integer'],
            'images' => ['array', 'max:3'],
            'images.*' => ['file', 'mimetypes:image/jpeg,image/png,image/webp,image/gif', 'max:2048'],
        ];
    }
}
