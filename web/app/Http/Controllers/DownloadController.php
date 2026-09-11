<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Enums\ReleasePlatformEnum;
use App\Models\Release;
use Illuminate\Http\RedirectResponse;

/**
 * O link do site é fixo por plataforma; a URL assinada do bucket nasce aqui, na hora, e
 * vence em uma hora. O instalador nunca fica no disco do servidor.
 */
final class DownloadController
{
    public function __invoke(string $slug): RedirectResponse
    {
        $platform = ReleasePlatformEnum::fromSlug($slug);

        abort_if(is_null($platform), 404);

        $release = Release::latestPerPlatform()[$platform->value] ?? null;

        abort_if(is_null($release), 404);

        return redirect()->away($release->downloadUrl());
    }
}
