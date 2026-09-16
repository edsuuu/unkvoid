<?php

declare(strict_types=1);

namespace App\Http\Resources\Api;

use Carbon\CarbonImmutable;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use JsonException;

/**
 * Uma linha do histórico do servidor, venha da tabela do pacote de auditoria ou da dos
 * canais. A frase sai pronta daqui porque só o Laravel sabe o que cada coluna significa;
 * o `id` leva a letra da fonte porque as duas tabelas contam a partir de 1.
 *
 * @property-read int $id
 * @property-read string $source
 * @property-read ?int $actor_id
 * @property-read ?string $actor_name
 * @property-read string $event
 * @property-read string $type
 * @property-read ?string $old_values
 * @property-read ?string $new_values
 * @property-read string $created_at
 */
final class AuditResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     *
     * @throws JsonException
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->source.$this->id,
            'at' => CarbonImmutable::parse($this->created_at)->toIso8601String(),
            'event' => $this->event,
            'type' => class_basename($this->type),
            'actor' => is_null($this->actor_id) ? null : ['id' => $this->actor_id, 'name' => $this->actor_name],
            'summary' => $this->summary(),
        ];
    }

    /**
     * @throws JsonException
     */
    private function summary(): string
    {
        $type = class_basename($this->type);

        // O membro não tem nome nos valores gravados, e cargo dele é sincronizado, não editado.
        if ($type === 'ServerMember') {
            return match ($this->event) {
                'created' => 'adicionou um membro',
                'deleted' => 'removeu um membro',
                'sync', 'attach', 'detach' => 'mudou os cargos de um membro',
                default => 'mudou um membro',
            };
        }

        $name = $this->name();
        $verb = match ($this->event) {
            'created' => 'criou',
            'deleted' => 'apagou',
            default => 'mudou',
        };

        $subject = match ($type) {
            'Channel' => is_null($name) ? 'o canal' : 'o canal #'.$name,
            'ServerRole' => is_null($name) ? 'o cargo' : 'o cargo '.$name,
            'Server' => is_null($name) ? 'o servidor' : 'o servidor '.$name,
            'Message' => 'uma mensagem',
            default => $type,
        };

        return $verb.' '.$subject;
    }

    /**
     * O nome de depois quando houve; o de antes serve para o que foi apagado.
     *
     * @throws JsonException
     */
    private function name(): ?string
    {
        $name = [...$this->decode($this->old_values), ...$this->decode($this->new_values)]['name'] ?? null;

        return is_string($name) ? $name : null;
    }

    /**
     * @return array<array-key, mixed>
     *
     * @throws JsonException
     */
    private function decode(?string $values): array
    {
        if (is_null($values) || $values === '') {
            return [];
        }

        $decoded = json_decode($values, true, 512, JSON_THROW_ON_ERROR);

        return is_array($decoded) ? $decoded : [];
    }
}
