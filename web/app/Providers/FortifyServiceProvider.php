<?php

declare(strict_types=1);

namespace App\Providers;

use App\Actions\Fortify\CreateNewUser;
use App\Actions\Fortify\ResetUserPassword;
use Illuminate\Cache\RateLimiting\Limit;
use Illuminate\Contracts\View\Factory;
use Illuminate\Contracts\View\View;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\RateLimiter;
use Illuminate\Support\ServiceProvider;
use Illuminate\Support\Str;
use Laravel\Fortify\Fortify;
use Override;

final class FortifyServiceProvider extends ServiceProvider
{
    /**
     * Register any application services.
     */
    #[Override]
    public function register(): void
    {
        //
    }

    /**
     * Bootstrap any application services.
     */
    public function boot(): void
    {
        $this->configureActions();
        $this->configureViews();
        $this->configureRateLimiting();
    }

    /**
     * Configure Fortify actions.
     */
    private function configureActions(): void
    {
        Fortify::resetUserPasswordsUsing(ResetUserPassword::class);
        Fortify::createUsersUsing(CreateNewUser::class);
    }

    /**
     * Configure Fortify views.
     */
    private function configureViews(): void
    {
        Fortify::loginView(fn (): Factory|View => view('auth.login'));
        Fortify::verifyEmailView(fn (): Factory|View => view('auth.verify-email'));
        Fortify::twoFactorChallengeView(fn (): Factory|View => view('auth.two-factor-challenge'));
        Fortify::confirmPasswordView(fn (): Factory|View => view('auth.confirm-password'));
        Fortify::registerView(fn (): Factory|View => view('auth.register'));
        Fortify::resetPasswordView(fn (): Factory|View => view('auth.reset-password'));
        Fortify::requestPasswordResetLinkView(fn (): Factory|View => view('auth.forgot-password'));
    }

    /**
     * Configure rate limiting.
     */
    private function configureRateLimiting(): void
    {
        RateLimiter::for('two-factor', function (Request $request) {
            $loginId = $request->session()->get('login.id');

            return Limit::perMinute(5)->by(is_scalar($loginId) ? (string) $loginId : '');
        });

        RateLimiter::for('login', function (Request $request) {
            $username = $request->input(Fortify::username());
            $username = is_string($username) ? $username : '';

            $throttleKey = Str::transliterate(Str::lower($username).'|'.$request->ip());

            return Limit::perMinute(5)->by($throttleKey);
        });
    }
}
