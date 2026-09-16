<?php

declare(strict_types=1);

namespace App\Services\Storage;

use Aws\S3\Exception\S3Exception;
use Illuminate\Filesystem\AwsS3V3Adapter;
use Illuminate\Support\Facades\Config;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Storage;

final class BucketService
{
    /**
     * Cria o bucket quando ele ainda não existe, privado como o padrão do S3: numa máquina nova
     * o primeiro clipe e a primeira versão publicada não podem falhar por falta dele. Devolve
     * se criou.
     *
     * @throws S3Exception
     */
    public function ensure(): bool
    {
        $disk = Storage::disk('s3');

        // Disco local (ou o falso dos testes) não tem bucket para criar.
        if (! $disk instanceof AwsS3V3Adapter) {
            return false;
        }

        $client = $disk->getClient();
        $bucket = Config::string('filesystems.disks.s3.bucket');

        if ($client->doesBucketExistV2($bucket)) {
            return false;
        }

        try {
            $client->createBucket(['Bucket' => $bucket]);
        } catch (S3Exception $exception) {
            // Dois pedidos ao mesmo tempo: quem perde a corrida acha o bucket já criado.
            if ($exception->getAwsErrorCode() === 'BucketAlreadyOwnedByYou') {
                return false;
            }

            Log::channel('daily')->error('[ERRO] não deu para criar o bucket', [
                'bucket' => $bucket,
                'exception' => $exception,
                'message' => $exception->getMessage(),
            ]);

            throw $exception;
        }

        Log::channel('daily')->info('[INFO] bucket criado', ['bucket' => $bucket]);

        return true;
    }
}
