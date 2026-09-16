<?php

declare(strict_types=1);

namespace App\Console\Commands;

use App\Services\Storage\BucketService;
use Illuminate\Console\Command;

final class CreateBucketCommand extends Command
{
    protected $signature = 'storage:bucket';

    protected $description = 'Cria o bucket do S3/MinIO (AWS_BUCKET) se ele ainda não existe';

    public function handle(BucketService $bucket): int
    {
        $this->info($bucket->ensure() ? 'Bucket criado.' : 'O bucket já existia.');

        return self::SUCCESS;
    }
}
