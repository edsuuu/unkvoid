<?php

declare(strict_types=1);

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Auth;
use Symfony\Component\HttpFoundation\Response;

final class RequireNickname
{
    public function handle(Request $request, Closure $next): Response
    {
        $user = Auth::user();

        if (! $user || filled($user->nickname) || $request->routeIs('onboarding')) {
            return $next($request);
        }

        return redirect()->route('onboarding');
    }
}
