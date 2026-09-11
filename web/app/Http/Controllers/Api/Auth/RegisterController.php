<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Auth;

use App\Http\Requests\Api\Auth\RegisterRequest;
use App\Http\Resources\Api\AuthTokenResource;
use App\Models\User;
use App\Notifications\WelcomeNotification;
use Illuminate\Support\Facades\Log;
use Throwable;

final class RegisterController
{
    /**
     * @throws Throwable
     */
    public function __invoke(RegisterRequest $request): AuthTokenResource
    {
        $email = mb_strtolower(mb_trim($request->string('email')->toString()));

        try {
            $user = User::query()->create([
                'name' => mb_trim($request->string('name')->toString()),
                'email' => $email,
                'password' => $request->string('password')->toString(),
            ]);
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao criar a conta pela API', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'email' => $email,
            ]);

            throw $exception;
        }

        $user->notifyQuietly(new WelcomeNotification);

        return new AuthTokenResource($user, $user->createToken($request->string('device')->toString())->plainTextToken);
    }
}
