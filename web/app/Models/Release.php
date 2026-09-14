<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\ReleasePlatformEnum;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Storage;
use Override;
use RuntimeException;
use Throwable;

/**
 * @property int $id
 * @property string $version
 * @property ReleasePlatformEnum $platform
 * @property string $file_name
 * @property string $path
 * @property int $size
 * @property ?string $signature
 * @property ?string $notes
 * @property CarbonImmutable $published_at
 */
#[Fillable(['version', 'platform', 'file_name', 'path', 'size', 'signature', 'notes', 'published_at'])]
final class Release extends Model
{
    /**
     * Sobe o instalador para o bucket e registra a versão. A mesma versão da mesma
     * plataforma publicada de novo substitui a anterior: é o caso de refazer um build.
     *
     * @throws Throwable
     */
    public static function publish(string $version, ReleasePlatformEnum $platform, UploadedFile $file, ?string $signature, ?string $notes): self
    {
        $fileName = $file->getClientOriginalName();
        $directory = "releases/{$version}";

        try {
            return DB::transaction(function () use ($version, $platform, $file, $signature, $notes, $fileName, $directory): self {
                $path = Storage::disk('s3')->putFileAs($directory, $file, $fileName);

                throw_if($path === false, RuntimeException::class, "não deu para gravar {$fileName} no bucket");

                return self::query()->updateOrCreate(
                    ['version' => $version, 'platform' => $platform->value],
                    [
                        'file_name' => $fileName,
                        'path' => $path,
                        'size' => $file->getSize(),
                        'signature' => $signature,
                        'notes' => $notes,
                        'published_at' => now(),
                    ],
                );
            });
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao publicar a versão', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'version' => $version,
                'platform' => $platform->value,
            ]);

            throw $exception;
        }
    }

    /**
     * @return array<string, self> a mais nova de cada plataforma, pela chave do Tauri
     */
    public static function latestPerPlatform(): array
    {
        /** @var array<string, self> $latest */
        $latest = [];

        foreach (self::query()->latest('published_at')->orderByDesc('id')->get() as $release) {
            $latest[$release->platform->value] ??= $release;
        }

        return $latest;
    }

    public function downloadUrl(): string
    {
        return Storage::disk('s3')->temporaryUrl($this->path, now()->addHour());
    }

    /**
     * @throws Throwable
     */
    public function remove(): void
    {
        try {
            DB::transaction(function (): void {
                Storage::disk('s3')->delete($this->path);
                $this->delete();
            });
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao apagar a versão', [
                'exception' => $exception,
                'message' => $exception->getMessage(),
                'release_id' => $this->id,
            ]);

            throw $exception;
        }
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'platform' => ReleasePlatformEnum::class,
            'size' => 'integer',
            'published_at' => 'datetime',
        ];
    }
}
