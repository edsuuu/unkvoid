<?php

declare(strict_types=1);

namespace App\Http\Requests\Api\Servers;

use Illuminate\Foundation\Http\FormRequest;

final class UpdateMemberRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'nickname' => ['sometimes', 'nullable', 'string', 'max:32'],
            'role_ids' => ['sometimes', 'array'],
            'role_ids.*' => ['integer'],
            'server_mute' => ['sometimes', 'boolean'],
            'server_deaf' => ['sometimes', 'boolean'],
        ];
    }

    /**
     * O `boolean` aceita também 1, 0, "1" e "0": o que segue para o modelo é sempre bool, senão
     * o `server_mute: 1` gravava o mudo e morria em TypeError antes de avisar o SFU.
     *
     * @return array{nickname?: ?string, role_ids?: array<int, int>, server_mute?: bool, server_deaf?: bool}
     */
    public function changes(): array
    {
        /** @var array{nickname?: ?string, role_ids?: array<int, int>, server_mute?: bool, server_deaf?: bool} $changes */
        $changes = $this->validated();

        foreach (['server_mute', 'server_deaf'] as $field) {
            if (array_key_exists($field, $changes)) {
                $changes[$field] = $this->boolean($field);
            }
        }

        return $changes;
    }
}
