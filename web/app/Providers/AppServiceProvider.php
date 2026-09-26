<?php

declare(strict_types=1);

namespace App\Providers;

use App\Models\User;
use App\Services\Auth\AppTokens;
use Carbon\CarbonImmutable;
use Illuminate\Auth\Access\Response;
use Illuminate\Cache\RateLimiting\Limit;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Date;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Gate;
use Illuminate\Support\Facades\RateLimiter;
use Illuminate\Support\ServiceProvider;
use Illuminate\Validation\Rules\Password;
use Laravel\Sanctum\PersonalAccessToken;
use Laravel\Sanctum\Sanctum;
use Override;

final class AppServiceProvider extends ServiceProvider
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
        $this->configureDefaults();

        Gate::define('viewLogViewer', fn (User $user) => $user->isAdmin()
            ? Response::allow()
            : Response::deny(__('This action is unauthorized.')));

        // O token de renovação não abre rota nenhuma: ele só troca o par em `/api/auth/refresh`,
        // que o procura por conta própria.
        Sanctum::authenticateAccessTokensUsing(
            static fn (PersonalAccessToken $token, bool $valid): bool => $valid && ! in_array(AppTokens::REFRESH, $token->abilities ?? [], true),
        );

        RateLimiter::for('login', function (Request $request): Limit {
            $email = mb_strtolower(mb_trim($request->string('email')->toString()));

            return Limit::perMinute(5)->by($email.'|'.$request->ip());
        });
    }

    /**
     * Configure default behaviors for production-ready applications.
     */
    private function configureDefaults(): void
    {
        Date::use(CarbonImmutable::class);

        DB::prohibitDestructiveCommands(
            app()->isProduction(),
        );

        Password::defaults(
            fn (): ?Password => app()->isProduction()
            ? Password::min(12)
                ->mixedCase()
                ->letters()
                ->numbers()
                ->symbols()
                ->uncompromised()
            : null,
        );
    }
}
