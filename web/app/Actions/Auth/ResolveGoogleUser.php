<?php

declare(strict_types=1);

namespace App\Actions\Auth;

use App\Models\User;
use Laravel\Socialite\Contracts\User as SocialiteUser;

final class ResolveGoogleUser
{
    public function handle(SocialiteUser $googleUser): User
    {
        $existing = User::where('google_id', $googleUser->getId())
            ->orWhere('email', $googleUser->getEmail())
            ->first();

        if ($existing) {
            $existing->forceFill([
                'google_id' => $googleUser->getId(),
                'avatar_url' => $googleUser->getAvatar(),
                'email_verified_at' => $existing->email_verified_at ?? now(),
            ])->save();

            return $existing;
        }

        return User::create([
            'name' => $googleUser->getName() ?? $googleUser->getNickname() ?? 'Usuário',
            'email' => $googleUser->getEmail(),
            'google_id' => $googleUser->getId(),
            'avatar_url' => $googleUser->getAvatar(),
            'email_verified_at' => now(),
        ]);
    }
}
