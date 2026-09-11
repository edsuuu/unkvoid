<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Enums\ReleasePlatformEnum;
use App\Models\Release;
use Illuminate\Http\JsonResponse;

/**
 * O que o atualizador do Tauri lê. O formato é o dele: uma versão, e uma URL assinada
 * por plataforma. Sem cache de propósito: as URLs vencem, e o app baixa logo em seguida.
 */
final class ManifestController
{
    public function __invoke(): JsonResponse
    {
        $latest = Release::latestPerPlatform();

        abort_if(empty($latest), 404);

        $newest = null;

        /** @var array<string, array{url: string, signature: string}> $platforms */
        $platforms = [];

        foreach ($latest as $release) {
            if (! $release->platform->updates() || is_null($release->signature)) {
                continue;
            }

            $platforms[$release->platform->value] = ['url' => $release->downloadUrl(), 'signature' => $release->signature];

            if (is_null($newest) || version_compare($release->version, $newest->version, '>')) {
                $newest = $release;
            }
        }

        abort_if(is_null($newest), 404);

        // O app que não sabe por qual instalador foi instalado procura só `windows-x86_64`.
        foreach ([ReleasePlatformEnum::WindowsNsis, ReleasePlatformEnum::WindowsMsi] as $fallback) {
            if (isset($platforms[$fallback->value])) {
                $platforms['windows-x86_64'] = $platforms[$fallback->value];

                break;
            }
        }

        return response()->json([
            'version' => $newest->version,
            'notes' => (string) $newest->notes,
            'pub_date' => $newest->published_at->toIso8601String(),
            'platforms' => $platforms,
        ], 200, [], JSON_UNESCAPED_SLASHES)->header('Cache-Control', 'no-store');
    }
}
