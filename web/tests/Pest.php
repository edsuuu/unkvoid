<?php

declare(strict_types=1);

use App\Models\Server;
use App\Models\ServerMember;
use App\Models\ServerRole;
use App\Models\User;
use Aws\CommandInterface;
use Aws\Result;
use Aws\S3\Exception\S3Exception;
use GuzzleHttp\Promise\Create;
use GuzzleHttp\Promise\PromiseInterface;
use GuzzleHttp\Psr7\Response;
use Illuminate\Foundation\Testing\RefreshDatabase;
use Illuminate\Support\Facades\Config;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Storage;
use Tests\TestCase;

pest()->extend(TestCase::class)
    ->use(RefreshDatabase::class)
    ->beforeEach(function (): void {
        // Nenhum teste fala com o MinIO: todo comando do S3 é respondido aqui.
        fakeS3Client();

        // O tempo real sai por HTTP em quase toda escrita: a publicação responde vazio em
        // todo teste, e o stub mais específico vem primeiro para não tapar o `fakeSfu()`.
        Http::fake(['*/broadcast' => Http::response()]);
    })
    ->in('Feature');

function joinServer(Server $server, User $user): ServerMember
{
    return $server->members()->create(['user_id' => $user->id, 'joined_at' => now()]);
}

function giveRole(Server $server, User $user, int $permissions, int $position = 1): ServerRole
{
    $role = $server->roles()->create(['name' => 'Cargo '.$position, 'position' => $position, 'permissions' => $permissions]);
    $server->members()->where('user_id', $user->id)->firstOrFail()->roles()->attach($role);

    return $role;
}

function fakeSfu(string $room = '', User ...$users): void
{
    $peers = [];

    foreach ($users as $user) {
        $peers[] = ['sub' => "user:{$user->id}", 'name' => $user->name, 'sources' => ['mic']];
    }

    Http::fake([
        '*/presence' => Http::response(['rooms' => $room === '' ? [] : [$room => $peers]]),
        '*' => Http::response(['kicked' => 1, 'muted' => 1]),
    ]);
}

/**
 * @param  array<string, mixed>  $body
 * @return array<string, string>
 */
function sfuHeaders(array $body, string $path = '/api/sfu/events', ?string $secret = null): array
{
    $timestamp = (string) time();
    $json = json_encode($body, JSON_THROW_ON_ERROR);

    return [
        'X-Unkvoid-Timestamp' => $timestamp,
        'X-Unkvoid-Signature' => hash_hmac('sha256', "{$timestamp}\nPOST\n{$path}\n{$json}", $secret ?? config('services.sfu.secret')),
    ];
}

/**
 * O cliente S3 dos testes: nenhum comando sai para a rede. `$bucketExists` responde o
 * `HeadBucket`, e a lista devolvida guarda o nome de cada comando pedido.
 *
 * @return ArrayObject<int, string>
 */
function fakeS3Client(bool $bucketExists = true): ArrayObject
{
    /** @var ArrayObject<int, string> $commands */
    $commands = new ArrayObject();

    Config::set('filesystems.disks.s3.handler', function (CommandInterface $command) use ($bucketExists, $commands): PromiseInterface {
        $commands->append($command->getName());

        if ($command->getName() === 'HeadBucket' && ! $bucketExists) {
            return Create::rejectionFor(new S3Exception('Not Found', $command, ['response' => new Response(404)]));
        }

        return Create::promiseFor(new Result([]));
    });
    Storage::forgetDisk('s3');

    return $commands;
}
