<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\Auth;

use App\Exceptions\InvalidCredentialsException;
use App\Http\Requests\Api\Auth\LoginRequest;
use App\Http\Resources\Api\AuthTokenResource;
use App\Models\User;
use App\Notifications\NewLoginNotification;
use Illuminate\Support\Facades\Hash;

final class LoginController
{
    /**
     * @throws InvalidCredentialsException
     */
    public function __invoke(LoginRequest $request): AuthTokenResource
    {
        $email = mb_strtolower(mb_trim($request->string('email')->toString()));
        $user = User::query()->where('email', $email)->first();

        if (is_null($user) || is_null($user->password) || ! Hash::check($request->string('password')->toString(), $user->password)) {
            throw new InvalidCredentialsException;
        }

        $device = $request->string('device')->toString();
        $user->notifyQuietly(new NewLoginNotification('app: '.$device, (string) $request->ip(), (string) $request->userAgent()));

        return new AuthTokenResource($user, $user->createToken($device)->plainTextToken);
    }
}
