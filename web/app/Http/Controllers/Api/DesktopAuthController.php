<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Actions\Auth\ResolveGoogleUser;
use App\Http\Controllers\Controller;
use Illuminate\Http\RedirectResponse;
use Illuminate\Support\Facades\Log;
use Laravel\Socialite\Facades\Socialite;
use Symfony\Component\HttpFoundation\RedirectResponse as SymfonyRedirect;
use Throwable;

/**
 * Desktop app login through Google.
 *
 * The app has no browser session, so the flow is: open the system browser,
 * the user signs in with Google, and the callback returns the token via deep link
 * (`discord2://auth?token=...`). The token never passes through the app window.
 */
final class DesktopAuthController extends Controller
{
    public function redirect(): SymfonyRedirect
    {
        return Socialite::driver('google')
            ->redirectUrl(route('api.desktop.google.callback'))
            ->redirect();
    }

    public function callback(ResolveGoogleUser $resolveGoogleUser): RedirectResponse
    {
        try {
            $googleUser = Socialite::driver('google')
                ->redirectUrl(route('api.desktop.google.callback'))
                ->user();
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] desktop Google login failed', ['exception' => $exception]);

            return redirect()->away('discord2://auth?erro='.urlencode(__('Unable to sign in with Google.')));
        }

        $user = $resolveGoogleUser->handle($googleUser);
        $token = $user->createToken('desktop-google')->plainTextToken;

        return redirect()->away('discord2://auth?token='.urlencode($token));
    }
}
