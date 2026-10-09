<?php

declare(strict_types=1);

use App\Enums\ChannelTypeEnum;
use App\Enums\PermissionEnum;
use App\Events\VoiceStateUpdated;
use App\Models\Channel;
use App\Models\ChannelAccess;
use App\Models\GuestAccess;
use App\Models\Server;
use App\Models\User;
use GuzzleHttp\Promise\PromiseInterface;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\Http;

it('emite o token com o mesmo HMAC do SFU e o que a pessoa pode fazer', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id);

    $response = $this->actingAs($member, 'sanctum')
        ->withHeader('User-Agent', 'Unkvoid/0.1 (linux)')
        ->postJson("/api/channels/{$voice->id}/voice/token")
        ->assertOk()
        ->assertJsonPath('data.url', 'ws://127.0.0.1:3000/sfu')
        ->assertJsonPath('data.expires_in', 60);

    [$body, $signature] = explode('.', (string) $response->json('data.token'));

    expect(hash_equals(hash_hmac('sha256', $body, (string) config('services.sfu.secret')), $signature))->toBeTrue();

    $claims = json_decode(base64_decode(strtr($body, '-_', '+/'), true), true, 512, JSON_THROW_ON_ERROR);

    expect($claims['room'])->toBe($voice->id)
        ->and($claims['sub'])->toBe("user:{$member->id}")
        ->and($claims['name'])->toBe($member->name)
        ->and($claims['can'])->toBe(['speak', 'stream', 'video'])
        ->and($claims['exp'])->toBeGreaterThan(time() + 50)->toBeLessThanOrEqual(time() + 60);

    $access = ChannelAccess::query()->firstOrFail();

    expect($access->user_id)->toBe($member->id)
        ->and($access->channel_id)->toBe($voice->id)
        ->and($access->user_agent)->toBe('Unkvoid/0.1 (linux)')
        ->and($access->left_at)->toBeNull();
});

it('mutado no servidor leva speak e a claim muted — quem cala é o SFU —, e sem CONNECT não leva token', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member)->update(['server_mute' => true]);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $text = $server->channels()->where('type', 'text')->firstOrFail();
    fakeSfu($voice->id);

    $token = (string) $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk()->json('data.token');
    $claims = json_decode(base64_decode(strtr(explode('.', $token)[0], '-_', '+/'), true), true, 512, JSON_THROW_ON_ERROR);

    expect($claims['can'])->toBe(['speak', 'stream', 'video'])
        ->and($claims['muted'])->toBeTrue();

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$text->id}/voice/token")->assertForbidden();

    $voice->overwrites()->create(['target_type' => 'role', 'target_id' => $server->everyoneRole()->id, 'allow' => 0, 'deny' => PermissionEnum::Connect->value]);

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();
});

it('canal com limite cheio recusa o token', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $voice->update(['user_limit' => 1]);
    fakeSfu($voice->id, $owner);

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertForbidden();

    $voice->update(['user_limit' => 2]);

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();
});

it('quem reconecta num canal cheio volta, e o navegador longo demais é cortado', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $voice->update(['user_limit' => 1]);
    fakeSfu($voice->id, $member);

    // O membro ainda consta na presença (a carência do SFU): pedir o token de novo não conta contra o limite.
    $this->actingAs($member, 'sanctum')->withHeader('User-Agent', str_repeat('a', 2000))->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();
    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertForbidden();

    expect(mb_strlen((string) ChannelAccess::query()->firstOrFail()->user_agent))->toBe(1023);
});

it('desconectar da voz exige MOVE_MEMBERS e hierarquia, e chama o kick do SFU', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id, $member);

    $this->actingAs($member, 'sanctum')->deleteJson("/api/channels/{$voice->id}/voice/members/{$owner->id}")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/channels/{$voice->id}/voice/members/{$member->id}")->assertNoContent();

    Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$voice->id}/kick") && $request['userId'] === "user:{$member->id}");
});

it('o webhook do SFU abre e fecha o acesso e retransmite o estado da voz', function (): void {
    Event::fake([VoiceStateUpdated::class]);

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id);

    $this->actingAs($owner, 'sanctum')->withHeader('User-Agent', 'Unkvoid/0.1')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();

    $joined = ['event' => 'joined', 'room' => $voice->id, 'sub' => "user:{$owner->id}", 'name' => $owner->name, 'ip' => '203.0.113.9', 'at' => time()];

    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();

    expect(ChannelAccess::query()->count())->toBe(2)
        ->and(ChannelAccess::query()->whereNull('left_at')->count())->toBe(1);

    $open = ChannelAccess::query()->whereNull('left_at')->firstOrFail();

    expect($open->sfu_ip)->toBe('203.0.113.9')
        ->and($open->ip)->toBe('127.0.0.1')
        ->and($open->user_agent)->toBe('Unkvoid/0.1');

    Event::assertDispatched(VoiceStateUpdated::class, fn (VoiceStateUpdated $event): bool => $event->event === 'joined' && $event->userId === $owner->id && $event->channels() === ["channel.{$voice->id}"]);

    $left = [...$joined, 'event' => 'left', 'at' => time() + 90];

    $this->withHeaders(sfuHeaders($left))->postJson('/api/sfu/events', $left)->assertNoContent();

    expect(ChannelAccess::query()->whereNull('left_at')->count())->toBe(0);

    Event::assertDispatched(VoiceStateUpdated::class, fn (VoiceStateUpdated $event): bool => $event->event === 'left');
});

it('quem foi expulso ou banido e entra com um token antigo é derrubado da voz na hora', function (): void {
    Event::fake([VoiceStateUpdated::class]);

    $owner = User::factory()->create();
    $gone = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id);

    $joined = ['event' => 'joined', 'room' => $voice->id, 'sub' => "user:{$gone->id}", 'name' => $gone->name, 'ip' => '203.0.113.9', 'at' => time()];

    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();

    Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$voice->id}/kick") && $request['userId'] === "user:{$gone->id}");

    expect(ChannelAccess::query()->count())->toBe(0);

    Event::assertNotDispatched(VoiceStateUpdated::class);
});

it('a mesma assinatura do SFU não entra duas vezes', function (): void {
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $joined = ['event' => 'joined', 'room' => $voice->id, 'sub' => "user:{$owner->id}", 'name' => $owner->name, 'ip' => '203.0.113.9', 'at' => time()];
    $headers = sfuHeaders($joined);

    $this->withHeaders($headers)->postJson('/api/sfu/events', $joined)->assertNoContent();
    $this->withHeaders($headers)->postJson('/api/sfu/events', $joined)->assertUnauthorized();

    $this->assertDatabaseCount('channel_accesses', 1);
});

it('o webhook sem assinatura válida é recusado', function (): void {
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $body = ['event' => 'joined', 'room' => $voice->id, 'sub' => "user:{$owner->id}", 'name' => $owner->name, 'ip' => '203.0.113.9', 'at' => time()];

    $this->postJson('/api/sfu/events', $body)->assertUnauthorized();
    $this->withHeaders(sfuHeaders($body, secret: 'outro-segredo-com-mais-de-32-caracteres'))->postJson('/api/sfu/events', $body)->assertUnauthorized();
    $this->withHeaders([...sfuHeaders($body), 'X-Unkvoid-Timestamp' => (string) (time() - 600)])->postJson('/api/sfu/events', $body)->assertUnauthorized();

    config(['services.sfu.secret' => '']);

    $this->withHeaders(sfuHeaders($body, secret: 'x'))->postJson('/api/sfu/events', $body)->assertUnauthorized();

    $this->assertDatabaseCount('channel_accesses', 0);
});

it('acesso aberto há mais de um dia é fechado quando outro abre', function (): void {
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id);

    $stale = ChannelAccess::query()->create(['channel_id' => $voice->id, 'user_id' => $owner->id, 'ip' => '10.0.0.1', 'joined_at' => now()->subDays(2)]);

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();

    expect($stale->refresh()->left_at)->not->toBeNull();
});

it('o visitante da sala por código entra na auditoria com nome, sala e IP, sem avisar canal nenhum', function (): void {
    Event::fake([VoiceStateUpdated::class]);

    $joined = ['event' => 'joined', 'room' => 'sala-da-tela', 'sub' => 'guest:4f1c2b9e-0000-4000-8000-000000000001', 'name' => 'Visitante', 'ip' => '198.51.100.7', 'at' => time()];

    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();

    $access = GuestAccess::query()->sole();

    expect($access->room)->toBe('sala-da-tela')
        ->and($access->install_id)->toBe('4f1c2b9e-0000-4000-8000-000000000001')
        ->and($access->name)->toBe('Visitante')
        ->and($access->ip)->toBe('198.51.100.7')
        ->and($access->left_at)->toBeNull();

    $left = [...$joined, 'event' => 'left', 'at' => time() + 90];

    $this->withHeaders(sfuHeaders($left))->postJson('/api/sfu/events', $left)->assertNoContent();

    expect(GuestAccess::query()->whereNull('left_at')->count())->toBe(0)
        ->and(ChannelAccess::query()->count())->toBe(0);

    Event::assertNotDispatched(VoiceStateUpdated::class);
});

it('o webhook recusa visitante com sala ou instalação fora do formato', function (): void {
    foreach ([['room' => 'sala com espaço', 'sub' => 'guest:abc'], ['room' => 'sala-da-tela', 'sub' => 'guest:<script>'], ['room' => 'sala-da-tela', 'sub' => 'guest:'.str_repeat('a', 65)]] as $invalid) {
        $body = ['event' => 'joined', ...$invalid, 'name' => 'Visitante', 'ip' => '198.51.100.7', 'at' => time()];

        $this->withHeaders(sfuHeaders($body))->postJson('/api/sfu/events', $body)->assertUnprocessable();
    }

    $this->assertDatabaseCount('guest_accesses', 0);
});

it('logado, a sala por código dá um token com a conta, e o id de um canal não serve de código', function (): void {
    $user = User::factory()->create();

    $token = (string) $this->actingAs($user, 'sanctum')->postJson('/api/rooms/sala-da-tela/token')->assertOk()->json('data.token');
    [$body, $signature] = explode('.', $token);
    $claims = json_decode(base64_decode(strtr($body, '-_', '+/'), true), true, 512, JSON_THROW_ON_ERROR);

    expect(hash_equals(hash_hmac('sha256', $body, (string) config('services.sfu.secret')), $signature))->toBeTrue()
        ->and($claims['room'])->toBe('sala-da-tela')
        ->and($claims['sub'])->toBe("user:{$user->id}")
        ->and($claims['can'])->toBe(['speak', 'stream', 'video']);

    $this->actingAs($user, 'sanctum')->postJson('/api/rooms/'.str_repeat('a', 26).'/token')->assertNotFound();
    $this->actingAs($user, 'sanctum')->postJson('/api/rooms/-invalida/token')->assertNotFound();
    auth()->forgetGuards();
    $this->postJson('/api/rooms/sala-da-tela/token')->assertUnauthorized();
});

it('quem entra logado na sala por código vai para a auditoria com a conta', function (): void {
    $user = User::factory()->create();
    $joined = ['event' => 'joined', 'room' => 'sala-da-tela', 'sub' => "user:{$user->id}", 'name' => $user->name, 'ip' => '198.51.100.8', 'at' => time()];

    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();

    expect(GuestAccess::query()->sole()->install_id)->toBe("user:{$user->id}")
        ->and(ChannelAccess::query()->count())->toBe(0);
});

it('mover exige MOVE_MEMBERS nos dois canais, hierarquia e a pessoa na voz, e avisa o SFU com o destino', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $origin = $server->channels()->where('type', 'voice')->firstOrFail();
    $destination = $server->createChannel($owner, 'Reunião', ChannelTypeEnum::Voice, null, 1);
    $text = $server->channels()->where('type', 'text')->firstOrFail();

    // O membro está na origem e o dono já ocupa a única vaga do destino.
    Http::fake([
        '*/presence' => Http::response(['rooms' => [
            $origin->id => [['sub' => "user:{$member->id}", 'name' => $member->name, 'sources' => ['mic']]],
            $destination->id => [['sub' => "user:{$owner->id}", 'name' => $owner->name, 'sources' => ['mic']]],
        ]]),
        '*' => Http::response(['kicked' => 1]),
    ]);

    $this->actingAs($member, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$owner->id}", ['channel_id' => $destination->id])->assertForbidden();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $text->id])->assertUnprocessable();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $origin->id])->assertUnprocessable();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$destination->id}/voice/members/{$member->id}", ['channel_id' => $origin->id])->assertNotFound();

    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $destination->id])->assertNoContent();

    Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$origin->id}/kick") && $request['userId'] === "user:{$member->id}" && $request['to'] === $destination->id && $request['by'] === $owner->name);

    // O passe faz o token do destino pular o limite, e vale os 60 s: a reconexão pede token de novo.
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$destination->id}/voice/token")->assertOk();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$destination->id}/voice/token")->assertOk();
    $this->travel(61)->seconds();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$destination->id}/voice/token")->assertForbidden();
});

it('mover para canal que a pessoa não vê é recusado, quem move precisa de CONNECT no destino, e o passe pula o CONNECT do movido', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $manager = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    joinServer($server, $manager);
    giveRole($server, $manager, PermissionEnum::MoveMembers->value, 3);
    $origin = $server->channels()->where('type', 'voice')->firstOrFail();
    $destination = $server->createChannel($owner, 'Trancado', ChannelTypeEnum::Voice, null, null);
    $everyone = $server->everyoneRole()->id;
    fakeSfu($origin->id, $member);

    $destination->overwrites()->create(['target_type' => 'role', 'target_id' => $everyone, 'allow' => 0, 'deny' => PermissionEnum::ViewChannel->value]);

    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $destination->id])->assertForbidden();

    $destination->overwrites()->delete();
    $destination->overwrites()->create(['target_type' => 'role', 'target_id' => $everyone, 'allow' => 0, 'deny' => PermissionEnum::Connect->value]);
    $destination->unsetRelation('overwrites');

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$destination->id}/voice/token")->assertForbidden();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $destination->id])->assertForbidden();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $destination->id])->assertNoContent();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$destination->id}/voice/token")->assertOk();
});

/**
 * A presença que muda no meio do teste: os stubs do `Http::fake` empilham e o primeiro que
 * casa responde, então um só, lendo a variável, no lugar de um `fakeSfu()` por cenário.
 *
 * @param  array<string, array<int, User>>  $rooms
 */
function livePresence(array &$rooms): void
{
    Http::fake([
        '*/presence' => function () use (&$rooms): PromiseInterface {
            $payload = [];

            foreach ($rooms as $room => $users) {
                $payload[$room] = array_map(fn (User $user): array => ['sub' => "user:{$user->id}", 'name' => $user->name, 'sources' => ['mic']], $users);
            }

            return Http::response(['rooms' => $payload]);
        },
        '*' => Http::response(['kicked' => 1, 'muted' => 1]),
    ]);
}

/**
 * @return array<string, mixed>
 */
function sfuJoined(Channel $channel, User $user, string $event = 'joined', int $at = 0): array
{
    return ['event' => $event, 'room' => $channel->id, 'sub' => "user:{$user->id}", 'name' => $user->name, 'ip' => '203.0.113.9', 'at' => $at === 0 ? time() : $at];
}

it('mover de volta em menos de 60 s entra: a marca de saída do destino é apagada', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $a = $server->channels()->where('type', 'voice')->firstOrFail();
    $b = $server->createChannel($owner, 'B', ChannelTypeEnum::Voice, null, null);
    $rooms = [$a->id => [$member]];
    livePresence($rooms);

    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$a->id}/voice/members/{$member->id}", ['channel_id' => $b->id])->assertNoContent();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$b->id}/voice/token")->assertOk();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$a->id}/voice/token")->assertForbidden();

    $rooms = [$b->id => [$member]];
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$b->id}/voice/members/{$member->id}", ['channel_id' => $a->id])->assertNoContent();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$a->id}/voice/token")->assertOk();
});

it('quem foi movido para canal trancado ou cheio reconecta: o passe vale 60 s e, sentado, não passa de novo por CONNECT nem pelo limite', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $other = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    joinServer($server, $other);
    $origin = $server->channels()->where('type', 'voice')->firstOrFail();
    $stage = $server->createChannel($owner, 'Palco', ChannelTypeEnum::Voice, null, 1);
    $stage->overwrites()->create(['target_type' => 'role', 'target_id' => $server->everyoneRole()->id, 'allow' => 0, 'deny' => PermissionEnum::Connect->value]);
    $rooms = [$origin->id => [$member], $stage->id => [$other]];
    livePresence($rooms);

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$stage->id}/voice/token")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $stage->id])->assertNoContent();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$stage->id}/voice/token")->assertOk();

    // O SFU confirmou a entrada; a presença esconde quem está na carência.
    $joined = sfuJoined($stage, $member);
    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();
    $rooms = [$stage->id => [$other]];
    $this->travel(61)->seconds();

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$stage->id}/voice/token")->assertOk();

    // Reconectar não abre acesso novo: a linha do `joined` continua a valer.
    expect(ChannelAccess::query()->where('user_id', $member->id)->where('channel_id', $stage->id)->whereNull('left_at')->count())->toBe(1);

    // Saiu de vez: a próxima entrada volta a passar por CONNECT.
    $left = sfuJoined($stage, $member, 'left', time() + 5);
    $this->withHeaders(sfuHeaders($left))->postJson('/api/sfu/events', $left)->assertNoContent();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$stage->id}/voice/token")->assertForbidden();
});

it('quem caiu da rede num canal cheio volta ao lugar dele, mesmo com alguém novo dentro', function (): void {
    $owner = User::factory()->create();
    $alice = User::factory()->create();
    $bob = User::factory()->create();
    $carol = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $alice);
    joinServer($server, $bob);
    joinServer($server, $carol);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $voice->update(['user_limit' => 2]);

    $rooms = [$voice->id => [$alice, $bob]];
    livePresence($rooms);

    foreach ([$alice, $bob] as $user) {
        $joined = sfuJoined($voice, $user);
        $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();
    }

    // Alice cai (some da presença), Carol ocupa o lugar, e a retomada da Alice ainda entra.
    $rooms = [$voice->id => [$bob]];
    $this->actingAs($carol, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();
    $rooms = [$voice->id => [$bob, $carol]];
    $this->actingAs($alice, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();

    // Quem nunca sentou continua barrado pelo limite.
    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertForbidden();
});

it('quem entra mutado pelo servidor volta a falar quando desmutam: o SFU é avisado na chegada e em todo canal de voz', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member)->update(['server_mute' => true]);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $other = $server->createChannel($owner, 'Outra', ChannelTypeEnum::Voice, null, null);
    fakeSfu($voice->id, $member);

    $joined = sfuJoined($voice, $member);
    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();

    Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$voice->id}/mute") && $request['userId'] === "user:{$member->id}" && $request['muted'] === true);

    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['server_mute' => false])->assertOk();

    foreach ([$voice, $other] as $channel) {
        Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$channel->id}/mute") && $request['muted'] === false);
    }

    // Desmutado, o token sai sem a claim; o `speak` sempre esteve lá.
    $claims = json_decode(base64_decode(strtr(explode('.', (string) $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk()->json('data.token'))[0], '-_', '+/'), true), true, 512, JSON_THROW_ON_ERROR);

    expect($claims['can'])->toBe(['speak', 'stream', 'video'])
        ->and($claims)->not->toHaveKey('muted');
});

it('banir, expulsar e mutar chegam ao SFU em todo canal de voz, mesmo com a presença fora do ar ou a pessoa na carência', function (): void {
    $owner = User::factory()->create();
    $banned = User::factory()->create();
    $kicked = User::factory()->create();
    $muted = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $banned);
    joinServer($server, $kicked);
    joinServer($server, $muted);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $other = $server->createChannel($owner, 'Outra', ChannelTypeEnum::Voice, null, null);

    Http::fake([
        '*/presence' => Http::response('upstream timeout', 504),
        '*' => Http::response(['kicked' => 0, 'muted' => 0]),
    ]);

    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$banned->id}")->assertSuccessful();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/servers/{$server->id}/members/{$kicked->id}")->assertNoContent();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$muted->id}", ['server_mute' => true])->assertOk();

    foreach ([$voice, $other] as $channel) {
        foreach ([$banned, $kicked] as $user) {
            Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$channel->id}/kick") && $request['userId'] === "user:{$user->id}");
        }

        Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$channel->id}/mute") && $request['userId'] === "user:{$muted->id}" && $request['muted'] === true);
    }
});
