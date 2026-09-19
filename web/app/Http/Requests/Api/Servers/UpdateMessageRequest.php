<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

final class UpdateMessageRequest extends FormRequest
{
    /**
     * O corpo pode chegar vazio, e quem decide se isso vale é `Message::edit`, depois de
     * conferir a permissão: só mensagem com imagem fica sem texto. `present` impede que um
     * PATCH sem o campo apague o texto por engano.
     *
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'body' => ['present', 'nullable', 'string', 'max:2000'],
        ];
    }
}
