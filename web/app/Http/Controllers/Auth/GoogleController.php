<?php

declare(strict_types=1);

namespace App\Http\Controllers\Auth;

use App\Actions\Auth\ResolveGoogleUser;
use App\Http\Controllers\Controller;
use Illuminate\Http\RedirectResponse;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Laravel\Socialite\Facades\Socialite;
use Symfony\Component\HttpFoundation\RedirectResponse as SymfonyRedirect;
use Throwable;

final class GoogleController extends Controller
{
    public function redirect(): SymfonyRedirect
    {
        return Socialite::driver('google')->redirect();
    }

    public function callback(ResolveGoogleUser $resolveGoogleUser): RedirectResponse
    {
        try {
            $googleUser = Socialite::driver('google')->user();
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] Google callback failed', ['exception' => $exception]);

            return redirect()->route('login')->withErrors(['email' => __('Unable to sign in with Google.')]);
        }

        Auth::login($resolveGoogleUser->handle($googleUser), remember: true);

        return redirect()->intended(route('app'));
    }
}
