<?php

declare(strict_types=1);

use App\Enums\ClipStatusEnum;
use App\Enums\PermissionEnum;
use App\Events\ClipUpdated;
use App\Models\Channel;
use App\Models\Clip;
use App\Models\Server;
use App\Models\User;
use Illuminate\Http\Client\Request;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Facades\URL;
use Illuminate\Support\Str;

function voiceOf(Server $server): Channel
{
    return $server->channels()->where('type', 'voice')->firstOrFail();
}

function clipFor(User $user, ClipStatusEnum $status = ClipStatusEnum::Ready): Clip
{
    return Clip::query()->create([
        'user_id' => $user->id,
        'streamer_user_id' => $user->id,
        'streamer_name' => $user->name,
        'server_name' => 'Casa',
        'channel_name' => 'Geral',
        'status' => $status,
    ]);
}

/**
 * O disco falso não assina nada: a URL leva o vencimento que o código pediu, para o teste
 * conferir que ele existe e é de 2 h.
 */
function fakeS3(): void
{
    Storage::fake('s3');
    Storage::disk('s3')->buildTemporaryUrlsUsing(fn (string $path, DateTimeInterface $expiration): string => 'https://s3.unkvoid.test/'.$path.'?expires='.$expiration->getTimestamp());
}

it('clipa na voz: 202 processing, e o SFU recebe a política de upload presa ao prefixo com a mesma assinatura do kick', function (): void {
    $owner = User::factory()->create(['name' => 'Edsu']);
    $streamer = User::factory()->create(['name' => 'Fulano']);
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $streamer);
    $voice = voiceOf($server);
    Http::fake(['*/rooms/*/clips' => Http::response(['accepted' => true], 202)]);

    $response = $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$voice->id}/clips", ['user_id' => $streamer->id])
        ->assertStatus(202)
        ->assertJsonPath('data.status', 'processing')
        ->assertJsonPath('data.streamer', ['id' => $streamer->id, 'name' => 'Fulano'])
        ->assertJsonPath('data.server_name', 'Casa')
        ->assertJsonPath('data.channel_name', 'Geral')
        ->assertJsonPath('data.thumbnail_url', null)
        ->assertJsonPath('data.playlist_url', null)
        ->assertJsonPath('data.download_url', null);

    $id = (string) $response->json('data.id');
    $clip = Clip::query()->findOrFail($id);

    expect($id)->toMatch('/^[0-9a-z]{26}$/')
        ->and($clip->user_id)->toBe($owner->id)
        ->and($response->json('data.expires_at'))->toBe($clip->created_at->addDays(7)->toIso8601String());

    Http::assertSent(function (Request $request) use ($voice, $owner, $streamer, $id): bool {
        $path = "/rooms/{$voice->id}/clips";
        $signature = hash_hmac('sha256', $request->header('X-Unkvoid-Timestamp')[0]."\nPOST\n{$path}\n".$request->body(), (string) config('services.sfu.secret'));
        $policy = json_decode(base64_decode((string) $request['upload']['fields']['Policy'], true), true, 512, JSON_THROW_ON_ERROR);

        return str_ends_with((string) $request->url(), $path)
            && hash_equals($signature, $request->header('X-Unkvoid-Signature')[0])
            && $request['clipId'] === $id
            && $request['clipper'] === "user:{$owner->id}"
            && $request['streamer'] === "user:{$streamer->id}"
            && $request['upload']['prefix'] === "clips/{$id}/"
            && $request['upload']['url'] === 'http://127.0.0.1:9000/teste'
            && ! array_key_exists('key', $request['upload']['fields'])
            && in_array(['starts-with', '$key', "clips/{$id}/"], $policy['conditions'], true);
    });
});

it('canal de texto dá 422; sem CONNECT, sem VIEW_CHANNEL ou de outro servidor dá 403; nada chega ao SFU', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $stranger = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    Server::createFor($stranger, 'Outra casa');
    $voice = voiceOf($server);
    $text = $server->channels()->where('type', 'text')->firstOrFail();
    Http::fake();

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$text->id}/clips", ['user_id' => $member->id])->assertUnprocessable();
    $this->actingAs($stranger, 'sanctum')->postJson("/api/channels/{$voice->id}/clips", ['user_id' => $owner->id])->assertForbidden();

    $overwrite = $voice->overwrites()->create(['target_type' => 'role', 'target_id' => $server->everyoneRole()->id, 'allow' => 0, 'deny' => PermissionEnum::Connect->value]);

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/clips", ['user_id' => $owner->id])->assertForbidden();

    $overwrite->update(['deny' => PermissionEnum::ViewChannel->value]);

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/clips", ['user_id' => $owner->id])->assertForbidden();

    $this->assertDatabaseCount('clips', 0);
    Http::assertNothingSent();
});

it('SFU que recusa ou está fora não deixa a linha', function (int $sfuStatus, int $expected): void {
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = voiceOf($server);
    Http::fake(['*/rooms/*/clips' => $sfuStatus === 0 ? Http::failedConnection() : Http::response(['message' => 'não'], $sfuStatus)]);

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$voice->id}/clips", ['user_id' => $owner->id])->assertStatus($expected);

    $this->assertDatabaseCount('clips', 0);
})->with([
    'pessoa sem anel' => [404, 422],
    'quem clipa fora da sala' => [403, 403],
    'sem ffmpeg' => [503, 503],
    'SFU fora do ar' => [0, 503],
]);

it('só quem clipou lista, vê e apaga; de outra pessoa é 404', function (): void {
    $this->freezeSecond();
    fakeS3();
    $owner = User::factory()->create();
    $other = User::factory()->create();
    $older = clipFor($owner);
    $older->forceFill(['created_at' => now()->subDay()])->save();
    $clip = clipFor($owner);
    clipFor($other);
    Storage::disk('s3')->put("clips/{$clip->id}/index.m3u8", '#EXTM3U');
    Storage::disk('s3')->put("clips/{$clip->id}/seg-000.ts", 'video');
    $expires = now()->addHours(2)->getTimestamp();

    $this->actingAs($owner, 'sanctum')->getJson('/api/clips')->assertOk()
        ->assertJsonCount(2, 'data')
        ->assertJsonPath('data.0.id', $clip->id)
        ->assertJsonPath('data.1.id', $older->id)
        ->assertJsonPath('data.0.thumbnail_url', "https://s3.unkvoid.test/clips/{$clip->id}/thumb.jpg?expires={$expires}")
        ->assertJsonPath('data.0.download_url', "https://s3.unkvoid.test/clips/{$clip->id}/clip.mp4?expires={$expires}");

    $this->actingAs($other, 'sanctum')->getJson('/api/clips')->assertOk()->assertJsonCount(1, 'data');
    $this->actingAs($other, 'sanctum')->getJson("/api/clips/{$clip->id}")->assertNotFound();
    $this->actingAs($other, 'sanctum')->deleteJson("/api/clips/{$clip->id}")->assertNotFound();

    Storage::disk('s3')->assertExists("clips/{$clip->id}/seg-000.ts");

    $this->actingAs($owner, 'sanctum')->getJson("/api/clips/{$clip->id}")->assertOk()->assertJsonPath('data.status', 'ready');
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/clips/{$clip->id}")->assertNoContent();

    Storage::disk('s3')->assertMissing("clips/{$clip->id}/seg-000.ts");
    Storage::disk('s3')->assertMissing("clips/{$clip->id}/index.m3u8");
    $this->assertDatabaseMissing('clips', ['id' => $clip->id]);
});

it('nenhuma URL de clipe é pública: miniatura, download e playlist saem assinadas e vencem em 2 h', function (): void {
    $this->freezeSecond();
    $owner = User::factory()->create();
    $clip = clipFor($owner);

    $data = $this->actingAs($owner, 'sanctum')->getJson("/api/clips/{$clip->id}")->assertOk()->json('data');

    foreach (['thumbnail_url' => 'thumb.jpg', 'download_url' => 'clip.mp4'] as $field => $file) {
        parse_str((string) parse_url((string) $data[$field], PHP_URL_QUERY), $query);

        expect($data[$field])->toStartWith("http://127.0.0.1:9000/teste/clips/{$clip->id}/{$file}?")
            ->and($query)->toHaveKey('X-Amz-Signature')
            ->and((int) $query['X-Amz-Expires'])->toBeGreaterThan(7190)->toBeLessThanOrEqual(7200);
    }

    parse_str((string) parse_url((string) $data['download_url'], PHP_URL_QUERY), $download);
    parse_str((string) parse_url((string) $data['playlist_url'], PHP_URL_QUERY), $playlist);

    expect($download['response-content-disposition'])->toBe("attachment; filename=\"unkvoid-{$clip->id}.mp4\"")
        ->and($playlist)->toHaveKey('signature')
        ->and((int) $playlist['expires'])->toBe(now()->addHours(2)->getTimestamp());

    auth()->forgetGuards();
    $this->travel(121)->minutes();

    $this->get((string) $data['playlist_url'])->assertForbidden();
});

it('vencido some da lista e da API, e o prune apaga o objeto e a linha', function (): void {
    fakeS3();
    $owner = User::factory()->create();
    $expired = clipFor($owner);
    $expired->forceFill(['created_at' => now()->subDays(7)->subMinute()])->save();
    $fresh = clipFor($owner);
    Storage::disk('s3')->put("clips/{$expired->id}/seg-000.ts", 'video');
    Storage::disk('s3')->put("clips/{$fresh->id}/seg-000.ts", 'video');

    $this->actingAs($owner, 'sanctum')->getJson('/api/clips')->assertOk()->assertJsonCount(1, 'data')->assertJsonPath('data.0.id', $fresh->id);
    $this->actingAs($owner, 'sanctum')->getJson("/api/clips/{$expired->id}")->assertNotFound();
    $this->get(URL::temporarySignedRoute('api.clips.playlist', now()->addHour(), ['clip' => $expired->id]))->assertNotFound();

    $this->artisan('model:prune', ['--model' => [Clip::class]])->assertSuccessful();

    Storage::disk('s3')->assertMissing("clips/{$expired->id}/seg-000.ts");
    Storage::disk('s3')->assertExists("clips/{$fresh->id}/seg-000.ts");
    $this->assertDatabaseMissing('clips', ['id' => $expired->id]);
    $this->assertDatabaseHas('clips', ['id' => $fresh->id]);
});

it('clip.ready assinado atualiza, avisa quem clipou e a playlist troca os segmentos por URLs assinadas', function (): void {
    $this->freezeSecond();
    Event::fake([ClipUpdated::class]);
    fakeS3();
    $owner = User::factory()->create();
    $clip = clipFor($owner, ClipStatusEnum::Processing);
    Storage::disk('s3')->put("clips/{$clip->id}/index.m3u8", "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg-000.ts\n#EXTINF:4.0,\n../../releases/instalador.msi\n#EXT-X-ENDLIST\n");

    $ready = ['event' => 'clip.ready', 'clipId' => $clip->id, 'durationMs' => 300000, 'sizeBytes' => 187000000, 'at' => time()];

    $this->withHeaders(sfuHeaders($ready))->postJson('/api/sfu/events', $ready)->assertNoContent();

    $clip->refresh();

    expect($clip->status)->toBe(ClipStatusEnum::Ready)
        ->and($clip->duration_ms)->toBe(300000)
        ->and($clip->size_bytes)->toBe(187000000);

    Event::assertDispatched(ClipUpdated::class, fn (ClipUpdated $event): bool => $event->broadcastOn()->name === "private-user.{$owner->id}"
        && $event->broadcastAs() === 'ClipUpdated'
        && $event->broadcastWith()['clip']['status'] === 'ready'
        && ! is_null($event->broadcastWith()['clip']['playlist_url'])
        && ! is_null($event->broadcastWith()['clip']['download_url']));

    $playlistUrl = (string) $this->actingAs($owner, 'sanctum')->getJson("/api/clips/{$clip->id}")->assertOk()->json('data.playlist_url');
    $expires = now()->addHours(2)->getTimestamp();

    auth()->forgetGuards();

    $playlist = $this->get($playlistUrl)->assertOk()->assertHeader('Content-Type', 'application/vnd.apple.mpegurl');

    expect($playlist->getContent())->toBe(
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\n"
        ."https://s3.unkvoid.test/clips/{$clip->id}/seg-000.ts?expires={$expires}\n#EXTINF:4.0,\n"
        ."https://s3.unkvoid.test/clips/{$clip->id}/instalador.msi?expires={$expires}\n#EXT-X-ENDLIST\n",
    );

    $this->get((string) strtok($playlistUrl, '?'))->assertForbidden();
    $this->get($playlistUrl.'x')->assertForbidden();
});

it('playlist de clipe que ainda processa é 404 mesmo com assinatura', function (): void {
    fakeS3();
    $clip = clipFor(User::factory()->create(), ClipStatusEnum::Processing);

    $this->get(URL::temporarySignedRoute('api.clips.playlist', now()->addHour(), ['clip' => $clip->id]))->assertNotFound();
});

it('clip.failed marca a falha e avisa; webhook de clipe apagado limpa o prefixo; assinatura errada é recusada', function (): void {
    Event::fake([ClipUpdated::class]);
    fakeS3();
    $owner = User::factory()->create();
    $clip = clipFor($owner, ClipStatusEnum::Processing);

    $wrong = ['event' => 'clip.ready', 'clipId' => $clip->id, 'durationMs' => 1, 'sizeBytes' => 1, 'at' => time()];

    $this->withHeaders(sfuHeaders($wrong, secret: 'outro-segredo-com-mais-de-32-caracteres'))->postJson('/api/sfu/events', $wrong)->assertUnauthorized();

    expect($clip->refresh()->status)->toBe(ClipStatusEnum::Processing);

    $failed = ['event' => 'clip.failed', 'clipId' => $clip->id, 'reason' => 'ffmpeg saiu com 1', 'at' => time()];

    $this->withHeaders(sfuHeaders($failed))->postJson('/api/sfu/events', $failed)->assertNoContent();

    expect($clip->refresh()->status)->toBe(ClipStatusEnum::Failed);

    Event::assertDispatched(ClipUpdated::class, fn (ClipUpdated $event): bool => $event->broadcastWith()['clip']['status'] === 'failed'
        && is_null($event->broadcastWith()['clip']['playlist_url'])
        && is_null($event->broadcastWith()['clip']['download_url']));

    $gone = mb_strtolower((string) Str::ulid());
    Storage::disk('s3')->put("clips/{$gone}/seg-000.ts", 'video');
    $orphan = ['event' => 'clip.ready', 'clipId' => $gone, 'durationMs' => 1, 'sizeBytes' => 1, 'at' => time()];

    $this->withHeaders(sfuHeaders($orphan))->postJson('/api/sfu/events', $orphan)->assertNoContent();

    Storage::disk('s3')->assertMissing("clips/{$gone}/seg-000.ts");

    $invalid = ['event' => 'clip.ready', 'clipId' => '../../releases/xxxxxxxxxxxx', 'at' => time()];

    $this->withHeaders(sfuHeaders($invalid))->postJson('/api/sfu/events', $invalid)->assertUnprocessable();
});
