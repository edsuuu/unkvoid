<?php

declare(strict_types=1);

namespace App\Exceptions;

use Exception;
use Illuminate\Http\JsonResponse;

final class InvalidCredentialsException extends Exception
{
    public function render(): JsonResponse
    {
        return response()->json(['message' => 'E-mail ou senha não conferem.'], 401);
    }
}
