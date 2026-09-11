<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Enums\ReleasePlatformEnum;
use App\Http\Requests\Api\StoreReleaseRequest;
use App\Http\Resources\Api\ReleaseResource;
use App\Models\Release;
use Illuminate\Http\UploadedFile;
use Throwable;

final class ReleaseController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreReleaseRequest $request): ReleaseResource
    {
        $file = $request->file('file');

        abort_unless($file instanceof UploadedFile, 422);

        $release = Release::publish(
            $request->string('version')->toString(),
            ReleasePlatformEnum::from($request->string('platform')->toString()),
            $file,
            $request->filled('signature') ? $request->string('signature')->toString() : null,
            $request->filled('notes') ? $request->string('notes')->toString() : null,
        );

        return new ReleaseResource($release);
    }
}
