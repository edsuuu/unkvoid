<?php

declare(strict_types=1);

namespace App\Exceptions;

use Exception;
use Illuminate\Http\JsonResponse;

final class InvalidRefreshTokenException extends Exception
{
    public function render(): JsonResponse
    {
        return response()->json(['message' => 'Sua sessão terminou. Entre de novo.'], 401);
    }
}
