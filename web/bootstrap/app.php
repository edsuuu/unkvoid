<?php

declare(strict_types=1);

use App\Http\Middleware\VerifyReleaseSignature;
use App\Http\Middleware\VerifySfuSignature;
use Illuminate\Database\Eloquent\ModelNotFoundException;
use Illuminate\Foundation\Application;
use Illuminate\Foundation\Configuration\Exceptions;
use Illuminate\Foundation\Configuration\Middleware;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Symfony\Component\HttpKernel\Exception\NotFoundHttpException;

return Application::configure(basePath: dirname(__DIR__))
    ->withRouting(
        web: __DIR__.'/../routes/web.php',
        api: __DIR__.'/../routes/api.php',
        commands: __DIR__.'/../routes/console.php',
        health: '/up',
    )
    ->withBroadcasting(__DIR__.'/../routes/channels.php', ['middleware' => ['auth:sanctum']])
    ->withMiddleware(function (Middleware $middleware): void {
        $middleware->alias([
            'signed.release' => VerifyReleaseSignature::class,
            'signed.sfu' => VerifySfuSignature::class,
        ]);
    })
    ->withExceptions(function (Exceptions $exceptions): void {
        // O Laravel devolve a mensagem de toda exceção HTTP, e a do registro que não existe
        // leva o nome da classe junto: "No query results for model [App\Models\Server]".
        $exceptions->render(function (NotFoundHttpException $exception, Request $request): ?JsonResponse {
            if (! $request->expectsJson() || ! $exception->getPrevious() instanceof ModelNotFoundException) {
                return null;
            }

            return response()->json(['message' => 'Não encontrado.'], 404);
        });
    })->create();
