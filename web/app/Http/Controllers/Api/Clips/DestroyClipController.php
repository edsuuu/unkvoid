<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Clips;

use App\Models\Clip;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Response;
use Throwable;

final class DestroyClipController
{
    /**
     * @throws Throwable
     */
    public function __invoke(string $clip, #[CurrentUser] User $user): Response
    {
        Clip::findFor($user, $clip)->remove();

        return response()->noContent();
    }
}
