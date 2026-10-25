<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Presence\UpdateActivityRequest;
use App\Models\User;
use App\Services\Presence\ActivityPresence;
use App\Services\Sfu\SfuClient;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class ActivityController
{
    /**
     * @throws Throwable
     */
    public function update(UpdateActivityRequest $request, #[CurrentUser] User $user, ActivityPresence $presence, SfuClient $sfu): Response
    {
        /** @var array{name: string, started_at: string}|null $activity */
        $activity = $request->validated('activity');

        $presence->update($user, $activity, $sfu);

        return response()->noContent();
    }
}
