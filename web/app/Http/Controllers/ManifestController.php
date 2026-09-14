<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Enums\ReleasePlatformEnum;
use App\Models\Release;
use Illuminate\Http\JsonResponse;

/**
 * O que o atualizador do Tauri lê. O formato é o dele: uma versão, e uma URL assinada
 * por plataforma. Sem cache de propósito: as URLs vencem, e o app baixa logo em seguida.
 *
 * A versão é uma só para todas as plataformas, e por isso só entram aqui as que têm
 * build nessa versão. Anunciar 0.0.14 e servir o instalador 0.0.7 de uma plataforma que
 * ficou para trás faz o app dela baixar o antigo achando que é o novo: instala, volta a
 * ser velho, vê o anúncio de novo e repete para sempre.
 */
final class ManifestController
{
    public function __invoke(): JsonResponse
    {
        $latest = Release::latestPerPlatform();

        abort_if($latest === [], 404);

        $newest = null;

        foreach ($latest as $release) {
            if (! $release->platform->updates() || is_null($release->signature)) {
                continue;
            }

            if (is_null($newest) || version_compare($release->version, $newest->version, '>')) {
                $newest = $release;
            }
        }

        abort_if(is_null($newest), 404);

        /** @var array<string, array{url: string, signature: string}> $platforms */
        $platforms = [];

        foreach ($latest as $release) {
            if (! $release->platform->updates() || is_null($release->signature) || $release->version !== $newest->version) {
                continue;
            }

            $platforms[$release->platform->value] = ['url' => $release->downloadUrl(), 'signature' => $release->signature];
        }

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
