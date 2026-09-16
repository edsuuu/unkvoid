<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Enums\ReleasePlatformEnum;
use App\Models\Release;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\RedirectResponse;

/**
 * O que o site entrega para baixar: o manifesto que o atualizador lê e o instalador de cada sistema.
 */
final class DownloadController
{
    public function manifest(): JsonResponse
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

    public function platform(string $slug): RedirectResponse
    {
        $platform = ReleasePlatformEnum::fromSlug($slug);

        abort_if(is_null($platform), 404);

        $release = Release::latestPerPlatform()[$platform->value] ?? null;

        abort_if(is_null($release), 404);

        return redirect()->away($release->downloadUrl());
    }
}
