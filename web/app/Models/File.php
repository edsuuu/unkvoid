<?php

declare(strict_types=1);

namespace App\Models;

use App\Models\Concerns\LogsFailedWrites;
use App\Services\Storage\BucketService;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Storage;
use RuntimeException;
use Throwable;

/**
 * @property int $id
 * @property int $user_id
 * @property string $path
 * @property string $mime_type
 * @property int $size
 * @property ?CarbonImmutable $created_at
 * @property-read User $user
 */
#[Fillable(['user_id', 'path', 'mime_type', 'size'])]
final class File extends Model
{
    use LogsFailedWrites;

    private const int LINK_HOURS = 2;

    /**
     * Guarda no bucket e devolve a linha. A pasta separa o uso (`avatars`) e o id separa
     * quem enviou, para o bucket continuar navegável à mão.
     *
     * @throws Throwable
     */
    public static function put(User $owner, UploadedFile $upload, string $folder, BucketService $bucket): self
    {
        // Numa máquina nova o bucket pode não existir, e o arquivo falharia no upload.
        $bucket->ensure();

        $path = $upload->store($folder.'/'.$owner->id, 's3');

        throw_if($path === false, RuntimeException::class, 'não deu para guardar o arquivo');

        return self::write('falha ao gravar o arquivo enviado', fn (): self => self::query()->create([
            'user_id' => $owner->id,
            'path' => $path,
            'mime_type' => $upload->getMimeType() ?? 'application/octet-stream',
            'size' => $upload->getSize(),
        ]), ['path' => $path]);
    }

    /**
     * O bucket é privado: a URL sai assinada e vence.
     */
    public function url(): string
    {
        return Storage::disk('s3')->temporaryUrl($this->path, now()->addHours(self::LINK_HOURS));
    }

    /**
     * O arquivo do bucket sai depois da linha: falhar aqui deixa lixo no bucket, nunca uma
     * linha apontando para o vazio.
     *
     * @throws Throwable
     */
    public function forget(): void
    {
        self::write('falha ao apagar o arquivo enviado', fn () => $this->delete(), ['file_id' => $this->id]);

        try {
            Storage::disk('s3')->delete($this->path);
        } catch (Throwable $exception) {
            Log::channel('daily')->error('[ERRO] falha ao apagar o arquivo do bucket', [
                'file_id' => $this->id,
                'path' => $this->path,
                'exception' => $exception,
                'message' => $exception->getMessage(),
            ]);
        }
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }
}
