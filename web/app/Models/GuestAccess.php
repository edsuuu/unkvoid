<?php

declare(strict_types=1);

namespace App\Models;

use App\Models\Concerns\LogsFailedWrites;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Model;
use Override;
use Throwable;

/**
 * @property int $id
 * @property string $room
 * @property string $install_id
 * @property string $name
 * @property string $ip
 * @property CarbonImmutable $joined_at
 * @property ?CarbonImmutable $left_at
 */
#[Fillable(['room', 'install_id', 'name', 'ip', 'joined_at', 'left_at'])]
final class GuestAccess extends Model
{
    use LogsFailedWrites;

    /**
     * Uma sessão sem `left` depois disso é um webhook perdido, não alguém em chamada.
     */
    private const int STALE_HOURS = 24;

    /**
     * Mesmo formato do acesso de conta: fecha o que estava aberto para a mesma instalação
     * na mesma sala, e a limpeza dos acessos velhos anda junto, porque a VPS não roda
     * `schedule:run`.
     *
     * @throws Throwable
     */
    public static function open(string $room, string $installId, string $name, string $ip, CarbonImmutable $at): self
    {
        return self::write('falha ao registrar a entrada do visitante', function () use ($room, $installId, $name, $ip, $at): self {
            self::openFor($room, $installId)->update(['left_at' => $at]);
            self::query()->whereNull('left_at')->where('joined_at', '<', $at->subHours(self::STALE_HOURS))->update(['left_at' => $at]);

            return self::query()->create([
                'room' => $room,
                'install_id' => $installId,
                'name' => $name,
                'ip' => $ip,
                'joined_at' => $at,
            ]);
        }, ['room' => $room, 'install_id' => $installId]);
    }

    /**
     * Um `left` que chega atrasado só fecha o que abriu antes dele, nunca o acesso mais novo.
     *
     * @throws Throwable
     */
    public static function close(string $room, string $installId, CarbonImmutable $at): void
    {
        self::write('falha ao registrar a saída do visitante', fn () => self::openFor($room, $installId)->where('joined_at', '<', $at)->update(['left_at' => $at]), ['room' => $room, 'install_id' => $installId]);
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'joined_at' => 'datetime',
            'left_at' => 'datetime',
        ];
    }

    /**
     * @return Builder<self>
     */
    private static function openFor(string $room, string $installId): Builder
    {
        return self::query()->where('room', $room)->where('install_id', $installId)->whereNull('left_at');
    }
}
