<?php

declare(strict_types=1);

namespace App\Services\Presence;

use App\Events\ActivityUpdated;
use App\Models\User;
use App\Services\Sfu\SfuClient;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Log;
use Throwable;

final class ActivityPresence
{
    private const int TTL_SECONDS = 120;

    /**
     * @return array{name: string, started_at: string}|null
     */
    public function get(User $user): ?array
    {
        if ($user->publicStatus() === 'offline') {
            return null;
        }

        $activity = Cache::get($this->key($user->id));

        if (! is_array($activity)
            || ! is_string($activity['name'] ?? null)
            || ! is_string($activity['started_at'] ?? null)) {
            return null;
        }

        return [
            'name' => $activity['name'],
            'started_at' => $activity['started_at'],
        ];
    }

    /**
     * @param  array{name: string, started_at: string}|null  $activity
     *
     * @throws Throwable
     */
    public function update(User $user, ?array $activity, SfuClient $sfu): void
    {
        $activity = $user->publicStatus() === 'offline' ? null : $activity;
        $previous = Cache::get($this->key($user->id));
        $changed = $previous !== $activity;

        if (is_null($activity)) {
            Cache::forget($this->key($user->id));
        } else {
            Cache::put($this->key($user->id), $activity, now()->addSeconds(self::TTL_SECONDS));
        }

        $event = new ActivityUpdated($user, $activity);
        if (! $changed) {
            return;
        }

        event($event);

        foreach ($event->channels() as $channel) {
            try {
                $sfu->publish($channel, $event->eventName(), $event->payload());
            } catch (Throwable $exception) {
                Log::channel('daily')->warning('[ERRO] falha ao publicar atividade no tempo real', [
                    'exception' => $exception,
                    'user_id' => $user->id,
                    'channel' => $channel,
                ]);
            }
        }
    }

    private function key(int $user): string
    {
        return 'presence:activity:'.$user;
    }
}
