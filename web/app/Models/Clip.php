<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\ChannelTypeEnum;
use App\Enums\ClipStatusEnum;
use App\Enums\PermissionEnum;
use App\Events\ClipUpdated;
use App\Exceptions\ForbiddenException;
use App\Exceptions\SfuUnavailableException;
use App\Models\Concerns\LogsFailedWrites;
use App\Services\Sfu\SfuClient;
use App\Services\Storage\BucketService;
use Aws\S3\PostObjectV4;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Collection;
use Illuminate\Database\Eloquent\Concerns\HasUlids;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Prunable;
use Illuminate\Filesystem\AwsS3V3Adapter;
use Illuminate\Support\Facades\Config;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Facades\URL;
use Illuminate\Support\Str;
use Illuminate\Validation\ValidationException;
use Override;
use RuntimeException;
use Throwable;

/**
 * @property string $id
 * @property int $user_id
 * @property ?int $streamer_user_id
 * @property ?string $channel_id
 * @property string $streamer_name
 * @property string $server_name
 * @property string $channel_name
 * @property ClipStatusEnum $status
 * @property ?int $duration_ms
 * @property ?int $size_bytes
 * @property CarbonImmutable $created_at
 * @property CarbonImmutable $updated_at
 */
#[Fillable(['id', 'user_id', 'streamer_user_id', 'channel_id', 'streamer_name', 'server_name', 'channel_name', 'status', 'duration_ms', 'size_bytes'])]
final class Clip extends Model
{
    use HasUlids;
    use LogsFailedWrites;
    use Prunable;

    public const int KEEP_DAYS = 7;

    private const int LINK_HOURS = 2;

    private const int UPLOAD_MINUTES = 30;

    /**
     * A linha nasce antes da chamada ao SFU porque o `clip.ready` pode chegar antes da
     * resposta dele. Recusado ou fora do ar, ela sai; o que o SFU chegar a subir mesmo
     * assim some quando o webhook não achar o clipe.
     *
     * @throws Throwable
     */
    public static function start(User $clipper, Channel $channel, User $streamer, SfuClient $sfu, BucketService $bucket): self
    {
        $member = $channel->memberOrFail($clipper);
        $member->authorize(PermissionEnum::ViewChannel, $channel);

        if ($channel->type !== ChannelTypeEnum::Voice) {
            throw ValidationException::withMessages(['channel' => 'Só dá para clipar num canal de voz.']);
        }

        $member->authorize(PermissionEnum::Connect, $channel);

        // ponytail: a limpeza dos vencidos anda com o clipe novo porque a VPS não roda
        // `schedule:run`; sem clipe novo, o vencido some da API mas fica no bucket. Com o
        // agendador no ar, `model:prune` diário faz o mesmo sem mexer aqui.
        new self()->pruneAll();

        // Numa máquina nova o bucket pode não existir: criado aqui, o clipe não falha só no
        // upload do SFU, minutos depois, com um `NoSuchBucket` que ninguém vê.
        $bucket->ensure();

        $id = new self()->newUniqueId();
        $upload = self::uploadPolicy($id);

        $clip = self::write('falha ao criar o clipe', fn (): self => self::query()->create([
            'id' => $id,
            'user_id' => $clipper->id,
            'streamer_user_id' => $streamer->id,
            'channel_id' => $channel->id,
            'streamer_name' => $streamer->name,
            'server_name' => $channel->server->name,
            'channel_name' => $channel->name,
            'status' => ClipStatusEnum::Processing,
        ]), ['channel_id' => $channel->id, 'user_id' => $clipper->id]);

        $status = $sfu->clip($channel, $clip->id, $clipper, $streamer, $upload);

        if ($status >= 200 && $status < 300) {
            return $clip;
        }

        self::write('falha ao desfazer o clipe recusado', fn () => $clip->delete(), ['clip_id' => $clip->id, 'sfu_status' => $status]);

        throw match ($status) {
            404 => ValidationException::withMessages(['user_id' => 'Essa pessoa não está transmitindo neste canal.']),
            403 => new ForbiddenException('Você não está neste canal de voz.'),
            default => new SfuUnavailableException('Não deu para clipar agora. Tente de novo em instantes.'),
        };
    }

    /**
     * @return Collection<int, self>
     */
    public static function listFor(User $user): Collection
    {
        return self::alive()->where('user_id', $user->id)->latest()->orderByDesc('id')->get();
    }

    /**
     * Clipe de outra pessoa responde igual a clipe que não existe: 404.
     */
    public static function findFor(User $user, string $id): self
    {
        return self::alive()->where('user_id', $user->id)->findOrFail($id);
    }

    public static function findReady(string $id): self
    {
        return self::alive()->where('status', ClipStatusEnum::Ready)->findOrFail($id);
    }

    /**
     * @throws Throwable
     */
    public static function markReady(string $id, int $durationMs, int $sizeBytes): void
    {
        self::finish($id, ['status' => ClipStatusEnum::Ready, 'duration_ms' => $durationMs, 'size_bytes' => $sizeBytes]);
    }

    /**
     * @throws Throwable
     */
    public static function markFailed(string $id, string $reason): void
    {
        Log::channel('sfu')->warning('[WARN] o SFU não conseguiu gerar o clipe', ['clip_id' => $id, 'reason' => $reason]);

        self::finish($id, ['status' => ClipStatusEnum::Failed]);
    }

    /**
     * O id viaja para o SFU e vira prefixo no bucket: minúsculo como o do canal.
     */
    #[Override]
    public function newUniqueId(): string
    {
        return mb_strtolower((string) Str::ulid());
    }

    /**
     * @return Builder<self>
     */
    public function prunable(): Builder
    {
        return self::query()->where('created_at', '<=', now()->subDays(self::KEEP_DAYS));
    }

    /**
     * @throws Throwable
     */
    public function remove(): void
    {
        self::write('falha ao apagar o clipe', fn () => $this->prune(), ['clip_id' => $this->id]);
    }

    public function thumbnailUrl(): ?string
    {
        if ($this->status !== ClipStatusEnum::Ready) {
            return null;
        }

        return Storage::disk('s3')->temporaryUrl(self::directory($this->id).'thumb.jpg', now()->addHours(self::LINK_HOURS));
    }

    public function downloadUrl(): ?string
    {
        if ($this->status !== ClipStatusEnum::Ready) {
            return null;
        }

        return Storage::disk('s3')->temporaryUrl(self::directory($this->id).'clip.mp4', now()->addHours(self::LINK_HOURS), [
            'ResponseContentDisposition' => "attachment; filename=\"unkvoid-{$this->id}.mp4\"",
        ]);
    }

    public function playlistUrl(): ?string
    {
        if ($this->status !== ClipStatusEnum::Ready) {
            return null;
        }

        return URL::temporarySignedRoute('api.clips.playlist', now()->addHours(self::LINK_HOURS), ['clip' => $this->id]);
    }

    /**
     * O `index.m3u8` do bucket com cada segmento trocado por uma URL pré-assinada. O nome
     * do segmento passa por `basename`: é texto escrito pelo SFU, e um `../` ali assinaria
     * qualquer arquivo do bucket.
     */
    public function playlist(): ?string
    {
        $disk = Storage::disk('s3');
        $playlist = $disk->get(self::directory($this->id).'index.m3u8');

        if (is_null($playlist)) {
            return null;
        }

        $lines = [];

        foreach (explode("\n", $playlist) as $line) {
            $line = mb_trim($line);

            if ($line === '' || str_starts_with($line, '#')) {
                $lines[] = $line;

                continue;
            }

            $lines[] = $disk->temporaryUrl(self::directory($this->id).basename($line), now()->addHours(self::LINK_HOURS));
        }

        return implode("\n", $lines);
    }

    /**
     * @throws RuntimeException
     */
    protected function pruning(): void
    {
        throw_unless(Storage::disk('s3')->deleteDirectory(self::directory($this->id)), RuntimeException::class, 'não deu para apagar o clipe do bucket');
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'status' => ClipStatusEnum::class,
            'duration_ms' => 'integer',
            'size_bytes' => 'integer',
        ];
    }

    /**
     * @return Builder<self>
     */
    private static function alive(): Builder
    {
        return self::query()->where('created_at', '>', now()->subDays(self::KEEP_DAYS));
    }

    /**
     * Clipe que não existe mais foi apagado ou recusado enquanto o SFU trabalhava: o que
     * chegou ao bucket sai junto, senão fica lá sem linha que o apague.
     *
     * @param  array<string, mixed>  $changes
     *
     * @throws Throwable
     */
    private static function finish(string $id, array $changes): void
    {
        $clip = self::query()->find($id);

        if (is_null($clip)) {
            Storage::disk('s3')->deleteDirectory(self::directory($id));

            return;
        }

        self::write('falha ao atualizar o clipe', fn () => $clip->update($changes), ['clip_id' => $id]);

        self::broadcast(new ClipUpdated($clip));
    }

    /**
     * Uma política de POST do S3 presa ao prefixo do clipe: o SFU sobe os arquivos sem
     * nunca ter a credencial do bucket. O `key` sai dos campos porque é o SFU que o manda,
     * um por arquivo.
     *
     * @return array{url: string, fields: array<string, string>, prefix: string}
     */
    private static function uploadPolicy(string $id): array
    {
        $disk = Storage::disk('s3');

        throw_unless($disk instanceof AwsS3V3Adapter, RuntimeException::class, 'o disco s3 não é um bucket S3');

        $bucket = Config::string('filesystems.disks.s3.bucket');
        $prefix = self::directory($id);
        $post = new PostObjectV4($disk->getClient(), $bucket, [], [['bucket' => $bucket], ['starts-with', '$key', $prefix]], '+'.self::UPLOAD_MINUTES.' minutes');

        /** @var array<string, string> $fields */
        $fields = $post->getFormInputs();
        unset($fields['key']);

        /** @var array{action: string} $attributes */
        $attributes = $post->getFormAttributes();

        return ['url' => $attributes['action'], 'fields' => $fields, 'prefix' => $prefix];
    }

    private static function directory(string $id): string
    {
        return "clips/{$id}/";
    }
}
