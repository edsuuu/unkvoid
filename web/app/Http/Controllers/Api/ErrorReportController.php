<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\StoreErrorReportRequest;
use App\Http\Resources\Api\ErrorReportResource;
use App\Models\ErrorReport;
use Throwable;

final class ErrorReportController
{
    /**
     * @throws Throwable
     */
    public function __invoke(StoreErrorReportRequest $request): ErrorReportResource
    {
        $report = ErrorReport::record(
            $request->string('version')->toString(),
            $request->string('platform')->toString(),
            $request->string('log')->toString(),
        );

        return new ErrorReportResource($report);
    }
}
