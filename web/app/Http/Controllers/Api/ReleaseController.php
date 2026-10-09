<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Enums\ReleasePlatformEnum;
use App\Http\Requests\Api\StoreReleaseRequest;
use App\Http\Resources\Api\ReleaseResource;
use App\Models\Release;
use App\Services\Sfu\SfuClient;
use App\Services\Storage\BucketService;
use Illuminate\Http\UploadedFile;
use Throwable;

final class ReleaseController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreReleaseRequest $request, BucketService $bucket, SfuClient $sfu): ReleaseResource
    {
        $file = $request->file('file');

        abort_unless($file instanceof UploadedFile, 422);

        $version = $request->string('version')->toString();
        $platform = ReleasePlatformEnum::from($request->string('platform')->toString());

        $release = Release::publish(
            $version,
            $platform,
            $file,
            $request->filled('signature') ? $request->string('signature')->toString() : null,
            $request->filled('notes') ? $request->string('notes')->toString() : null,
            $bucket,
        );

        // O app aberto fica sabendo na hora, pelo socket do SFU que ele já usa, em vez de
        // perguntar ao site de tempos em tempos. Falha do SFU não desfaz a publicação: o
        // `publish` só deixa o rastro no log, e o app ainda confere ao abrir.
        $sfu->publish('releases', 'ReleasePublished', ['version' => $version, 'platform' => $platform->value]);

        return new ReleaseResource($release);
    }
}
