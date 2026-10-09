<?php

declare(strict_types=1);

// Auditoria de transmitir e assistir (docs/auditoria-transmissao.md), parte do Laravel. Ao
// contrário dos testes do Rust, cada teste aqui afirma o comportamento de HOJE: passar quer dizer
// que o defeito (REPRO) se reproduziu, ou que o código está certo (DISCARD). Quando um REPRO
// for corrigido, o teste dele passa a falhar: inverta a asserção e leve-o para o arquivo do
// assunto (tests/Feature/Servers/VoiceTest.php). Rodar: bash docs/auditoria/run-web.sh

use App\Enums\ChannelTypeEnum;
use App\Enums\PermissionEnum;
use App\Events\VoiceStateUpdated;
use App\Models\Channel;
use App\Models\ChannelAccess;
use App\Models\Server;
use App\Models\User;
use Illuminate\Http\Client\Request as ClientRequest;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Str;

function auditClaims(string $token): array
{
    return json_decode(base64_decode(strtr(explode('.', $token)[0], '-_', '+/'), true), true, 512, JSON_THROW_ON_ERROR);
}

/**
 * @param  array<string, array<int, User>>  $rooms
 */
function auditPresence(array $rooms, int $kicked = 1): void
{
    $payload = [];

    foreach ($rooms as $room => $users) {
        $payload[$room] = array_map(fn (User $user): array => ['sub' => "user:{$user->id}", 'name' => $user->name, 'sources' => ['mic']], $users);
    }

    // Http::fake stubs stack and the first match wins: one closure stub reads the current state.
    app()->instance('audit.presence', $payload);
    app()->instance('audit.kicked', $kicked);

    if (app()->bound('audit.faked')) {
        return;
    }

    app()->instance('audit.faked', true);

    Http::fake([
        '*/presence' => fn () => Http::response(['rooms' => app('audit.presence')]),
        '*' => fn () => Http::response(['kicked' => app('audit.kicked'), 'muted' => 1]),
    ]);
}

it('REPRO M1: moved A->B and back B->A within 60 s is refused at A by the stale move-out mark', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $a = $server->channels()->where('type', 'voice')->firstOrFail();
    $b = $server->createChannel($owner, 'B', ChannelTypeEnum::Voice, null, null);

    auditPresence([$a->id => [$member]]);
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$a->id}/voice/members/{$member->id}", ['channel_id' => $b->id])->assertNoContent();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$b->id}/voice/token")->assertOk();

    auditPresence([$b->id => [$member]]);
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$b->id}/voice/members/{$member->id}", ['channel_id' => $a->id])->assertNoContent();

    // The moderator just moved them INTO A; the app asks A's token and is refused.
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$a->id}/voice/token")
        ->assertForbidden()
        ->assertJsonPath('message', 'Você acabou de ser movido para outro canal.');
});

it('REPRO M2: moved into a channel without CONNECT, the reconnect token (resume after a blip) is refused', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $origin = $server->channels()->where('type', 'voice')->firstOrFail();
    $stage = $server->createChannel($owner, 'Palco', ChannelTypeEnum::Voice, null, null);
    $stage->overwrites()->create(['target_type' => 'role', 'target_id' => $server->everyoneRole()->id, 'allow' => 0, 'deny' => PermissionEnum::Connect->value]);

    auditPresence([$origin->id => [$member]]);
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $stage->id])->assertNoContent();

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$stage->id}/voice/token")->assertOk();

    // Now in the stage per the SFU. Wi-Fi blips; the app asks a token before the resume join.
    auditPresence([$stage->id => [$owner]]);
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$stage->id}/voice/token")->assertForbidden();
});

it('REPRO M2b: moved into a full channel, the reconnect token is refused (limit counted again)', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $other = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    joinServer($server, $other);
    $origin = $server->channels()->where('type', 'voice')->firstOrFail();
    $full = $server->createChannel($owner, 'Cheio', ChannelTypeEnum::Voice, null, 1);

    auditPresence([$origin->id => [$member], $full->id => [$other]]);
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $full->id])->assertNoContent();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$full->id}/voice/token")->assertOk();

    // The member is in (the SFU drops orphans from /presence, so they are not even listed).
    auditPresence([$full->id => [$other]]);
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$full->id}/voice/token")->assertForbidden()->assertJsonPath('message', 'O canal está cheio.');
});

it('REPRO M3: a move whose SFU kick hits nobody still answers 204, leaks the CONNECT-bypass pass and locks the origin', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $origin = $server->channels()->where('type', 'voice')->firstOrFail();
    $locked = $server->createChannel($owner, 'Trancado', ChannelTypeEnum::Voice, null, null);
    $locked->overwrites()->create(['target_type' => 'role', 'target_id' => $server->everyoneRole()->id, 'allow' => 0, 'deny' => PermissionEnum::Connect->value]);

    // Presence still lists the member, but by the time the kick lands they are gone (or the SFU timed out).
    auditPresence([$origin->id => [$member]], kicked: 0);

    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $locked->id])->assertNoContent();

    // Nobody was moved, yet: the member enters a channel they have no CONNECT on ...
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$locked->id}/voice/token")->assertOk();
    // ... and cannot go back to the channel they never left.
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$origin->id}/voice/token")->assertForbidden();
});

it('REPRO M4: two moderators moving the same person at once leave a pass for the destination that lost', function (): void {
    $owner = User::factory()->create();
    $mod = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $mod);
    joinServer($server, $member);
    $modRole = giveRole($server, $mod, PermissionEnum::MoveMembers->value, 5);
    $origin = $server->channels()->where('type', 'voice')->firstOrFail();
    $b = $server->createChannel($owner, 'B', ChannelTypeEnum::Voice, null, null);
    $c = $server->createChannel($owner, 'C', ChannelTypeEnum::Voice, null, null);
    // C is locked for @everyone; the mod's role is allowed back in.
    $c->overwrites()->create(['target_type' => 'role', 'target_id' => $server->everyoneRole()->id, 'allow' => 0, 'deny' => PermissionEnum::Connect->value]);
    $c->overwrites()->create(['target_type' => 'role', 'target_id' => $modRole->id, 'allow' => PermissionEnum::Connect->value, 'deny' => 0]);

    // Both requests read presence before either kick lands (the race window).
    auditPresence([$origin->id => [$member]]);
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $b->id])->assertNoContent();
    $this->actingAs($mod, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $c->id])->assertNoContent();

    // The SFU sent `moved {to: B}` (the second kick found nobody). The C pass is still live:
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$b->id}/voice/token")->assertOk();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$c->id}/voice/token")->assertOk();
});

it('REPRO L1: someone in the SFU grace loses their seat to a newcomer and is refused on resume', function (): void {
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

    // Alice and Bob are in. Alice's socket drops: RoomRegistry.presence() filters isOrphaned(),
    // so /presence lists only Bob while Alice is still a Peer inside her 30 s grace.
    auditPresence([$voice->id => [$bob]]);
    $this->actingAs($carol, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();

    // Carol joined; Alice's app asks a token for the resume.
    auditPresence([$voice->id => [$bob, $carol]]);
    $this->actingAs($alice, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertForbidden()->assertJsonPath('message', 'O canal está cheio.');
});

it('REPRO L2: after a move into a full channel, an original occupant cannot reconnect', function (): void {
    $owner = User::factory()->create();
    $alice = User::factory()->create();
    $mover = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $alice);
    joinServer($server, $mover);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $voice->update(['user_limit' => 1]);

    auditPresence([$voice->id => [$alice, $mover]]);
    $this->actingAs($alice, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertForbidden();
});

it('REPRO P1: a slow or failing /presence makes ban, kick and server-mute skip the SFU entirely', function (): void {
    $owner = User::factory()->create();
    $banned = User::factory()->create();
    $kicked = User::factory()->create();
    $muted = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $banned);
    joinServer($server, $kicked);
    joinServer($server, $muted);

    Http::fake([
        '*/presence' => Http::response('upstream timeout', 504),
        '*' => Http::response(['kicked' => 1, 'muted' => 1]),
    ]);

    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$banned->id}")->assertSuccessful();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/servers/{$server->id}/members/{$kicked->id}")->assertNoContent();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$muted->id}", ['server_mute' => true])->assertOk();

    // Everything answered success, and not one kick nor mute reached the SFU.
    Http::assertNotSent(fn (ClientRequest $request): bool => str_contains($request->url(), '/kick') || str_contains($request->url(), '/mute'));
});

it('REPRO P2: presence failure also fails the user_limit open', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $voice->update(['user_limit' => 1]);

    Http::fake(['*/presence' => Http::response('', 504), '*' => Http::response([])]);

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();
});

it('REPRO W1: a banned user who rejoins within the same second is not kicked again (replay guard eats the webhook)', function (): void {
    Event::fake([VoiceStateUpdated::class]);

    $owner = User::factory()->create();
    $gone = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id);

    $joined = ['event' => 'joined', 'room' => $voice->id, 'sub' => "user:{$gone->id}", 'name' => $gone->name, 'ip' => '203.0.113.9', 'at' => time()];

    // The SFU builds body and signature from (event, room, sub, name, ip, at-in-seconds):
    // a second join inside the same second produces the byte-identical request.
    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();
    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertUnauthorized();

    $kicks = 0;
    Http::assertSent(function (ClientRequest $request) use (&$kicks): bool {
        if (str_ends_with($request->url(), '/kick')) {
            $kicks++;
        }

        return true;
    });

    expect($kicks)->toBe(1);
});

it('REPRO W2: joined, left, joined in the same second: the second joined is dropped and the channel last hears `left`', function (): void {
    Event::fake([VoiceStateUpdated::class]);

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id);

    $now = time();
    $joined = ['event' => 'joined', 'room' => $voice->id, 'sub' => "user:{$owner->id}", 'name' => $owner->name, 'ip' => '203.0.113.9', 'at' => $now];
    $left = [...$joined, 'event' => 'left'];

    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();
    $this->withHeaders(sfuHeaders($left))->postJson('/api/sfu/events', $left)->assertNoContent();
    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertUnauthorized();

    $events = [];
    Event::assertDispatched(VoiceStateUpdated::class, function (VoiceStateUpdated $event) use (&$events): bool {
        $events[] = $event->event;

        return true;
    });

    expect($events)->toBe(['joined', 'left']);
});

it('REPRO S1: server_mute sent as 1 writes the mute and then dies with a TypeError before telling the SFU', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id, $member);

    $this->withoutExceptionHandling();

    expect(fn () => $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['server_mute' => 1]))
        ->toThrow(TypeError::class);

    expect($server->memberOf($member)->server_mute)->toBeTrue();
    Http::assertNotSent(fn (ClientRequest $request): bool => str_contains($request->url(), '/mute'));
});

it('REPRO S2: joined while server-muted, unmute only flips the SFU flag; the live token never had speak', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member)->update(['server_mute' => true]);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id, $member);

    $token = (string) $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk()->json('data.token');

    expect(auditClaims($token)['can'])->not->toContain('speak');

    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['server_mute' => false])->assertOk();

    // The only thing the SFU hears is /mute {muted:false}; the Peer's `can` (from the join) still
    // lacks `speak`, and Peer.assertCanProduce('mic') keeps refusing until the person rejoins.
    Http::assertSent(fn (ClientRequest $request): bool => str_ends_with($request->url(), "/rooms/{$voice->id}/mute") && $request['muted'] === false);
});

it('REPRO D1: deleting a voice channel leaves everyone in it streaming, and their `left` then 404s', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->createChannel($owner, 'Extra', ChannelTypeEnum::Voice, null, null);
    fakeSfu($voice->id, $member);

    $joined = ['event' => 'joined', 'room' => $voice->id, 'sub' => "user:{$member->id}", 'name' => $member->name, 'ip' => '203.0.113.9', 'at' => time()];
    $this->withHeaders(sfuHeaders($joined))->postJson('/api/sfu/events', $joined)->assertNoContent();

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/channels/{$voice->id}")->assertNoContent();

    Http::assertNotSent(fn (ClientRequest $request): bool => str_contains($request->url(), '/kick'));

    $left = [...$joined, 'event' => 'left', 'at' => time() + 5];
    $this->withHeaders(sfuHeaders($left))->postJson('/api/sfu/events', $left)->assertNotFound();
});

it('REPRO P1b: banning someone who is in the SFU grace (socket dropped) sends no kick: /presence hides orphans', function (): void {
    $owner = User::factory()->create();
    $banned = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $banned);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();

    // The Peer still exists in the room (RTP keeps flowing on the PlainTransport), but
    // RoomRegistry.presence() filters isOrphaned(), so Laravel sees an empty room.
    auditPresence([$voice->id => []]);

    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$banned->id}")->assertSuccessful();

    Http::assertNotSent(fn (ClientRequest $request): bool => str_contains($request->url(), '/kick'));
});

it('REPRO R1: a future-dated signed webhook can be replayed once its replay mark (300 s from first sight) expires', function (): void {
    Event::fake([VoiceStateUpdated::class]);

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id);

    $left = ['event' => 'left', 'room' => $voice->id, 'sub' => "user:{$owner->id}", 'name' => $owner->name, 'ip' => '203.0.113.9', 'at' => time()];
    $json = json_encode($left, JSON_THROW_ON_ERROR);
    $timestamp = (string) (time() + 290);
    $headers = ['X-Unkvoid-Timestamp' => $timestamp, 'X-Unkvoid-Signature' => hash_hmac('sha256', "{$timestamp}\nPOST\n/api/sfu/events\n{$json}", (string) config('services.sfu.secret'))];

    $this->withHeaders($headers)->postJson('/api/sfu/events', $left)->assertNoContent();
    $this->withHeaders($headers)->postJson('/api/sfu/events', $left)->assertUnauthorized();

    // 301 s later the mark is gone, while |now - timestamp| = 11 s is still inside the window.
    // (The middleware reads time() and the cache reads Carbon; travelling Carbon is that clock moving.)
    $this->travel(301)->seconds();

    $this->withHeaders($headers)->postJson('/api/sfu/events', $left)->assertNoContent();
});

// ---------------------------------------------------------------- DISCARD checks (correct behaviour)

it('DISCARD C1: `can` follows @everyone, aggregated roles, member overwrite, admin and owner', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $admin = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    joinServer($server, $admin);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $everyone = $server->everyoneRole()->id;
    $streamer = giveRole($server, $member, 0, 2);
    giveRole($server, $admin, PermissionEnum::Administrator->value, 3);
    fakeSfu($voice->id);

    $can = fn (User $user): array => auditClaims((string) $this->actingAs($user, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk()->json('data.token'))['can'];

    // @everyone denies STREAM and VIDEO; the member's role re-allows VIDEO; the member overwrite denies SPEAK.
    $voice->overwrites()->create(['target_type' => 'role', 'target_id' => $everyone, 'allow' => 0, 'deny' => PermissionEnum::Stream->value | PermissionEnum::Video->value]);
    $voice->overwrites()->create(['target_type' => 'role', 'target_id' => $streamer->id, 'allow' => PermissionEnum::Video->value, 'deny' => 0]);
    $voice->overwrites()->create(['target_type' => 'member', 'target_id' => $member->id, 'allow' => 0, 'deny' => PermissionEnum::Speak->value]);
    $voice->overwrites()->create(['target_type' => 'member', 'target_id' => $admin->id, 'allow' => 0, 'deny' => PermissionEnum::Speak->value | PermissionEnum::Stream->value]);
    $voice->overwrites()->create(['target_type' => 'member', 'target_id' => $owner->id, 'allow' => 0, 'deny' => PermissionEnum::Speak->value | PermissionEnum::Stream->value]);

    expect($can($member))->toBe(['video'])
        ->and($can($admin))->toBe(['speak', 'stream', 'video'])
        ->and($can($owner))->toBe(['speak', 'stream', 'video']);

    // Role deny beats @everyone allow; member allow beats role deny.
    $voice->overwrites()->delete();
    $voice->overwrites()->create(['target_type' => 'role', 'target_id' => $everyone, 'allow' => PermissionEnum::Stream->value, 'deny' => 0]);
    $voice->overwrites()->create(['target_type' => 'role', 'target_id' => $streamer->id, 'allow' => 0, 'deny' => PermissionEnum::Stream->value]);

    expect($can($member))->toBe(['speak', 'video']);

    $voice->overwrites()->create(['target_type' => 'member', 'target_id' => $member->id, 'allow' => PermissionEnum::Stream->value, 'deny' => 0]);

    expect($can($member))->toBe(['speak', 'stream', 'video']);
});

it('DISCARD C2: token is base64url without padding + 64 lowercase hex, room is lowercase ULID, exp is now+60', function (): void {
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    fakeSfu();

    foreach (range(1, 20) as $index) {
        $voice = $server->createChannel($owner, "V{$index}", ChannelTypeEnum::Voice, null, null);
        $before = time();
        $token = (string) $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk()->json('data.token');
        [$body, $signature] = explode('.', $token);

        expect($body)->toMatch('/^[A-Za-z0-9_-]+$/')
            ->and($signature)->toMatch('/^[0-9a-f]{64}$/')
            ->and(auditClaims($token)['room'])->toMatch('/^[a-z0-9]{26}$/')->toBe($voice->id)
            ->and(auditClaims($token)['exp'])->toBeGreaterThanOrEqual($before + 60)->toBeLessThanOrEqual(time() + 60)
            ->and(array_keys(auditClaims($token)))->toBe(['room', 'sub', 'name', 'exp', 'can']);
    }

    file_put_contents(getenv('AUDIT_OUT') ?: '/dev/null', json_encode(['token' => $token, 'secret' => config('services.sfu.secret')]));
});

it('DISCARD C3: the HTTP signature Laravel sends to the SFU is ts\nMETHOD\npath\nbody over the exact bytes sent', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id, $member);

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/channels/{$voice->id}/voice/members/{$member->id}")->assertNoContent();

    $captured = [];
    Http::assertSent(function (ClientRequest $request) use (&$captured): bool {
        $path = (string) parse_url($request->url(), PHP_URL_PATH);
        $captured[] = [
            'method' => $request->method(),
            'path' => $path,
            'body' => $request->body(),
            'ts' => $request->header('X-Unkvoid-Timestamp')[0],
            'sig' => $request->header('X-Unkvoid-Signature')[0],
        ];

        return true;
    });

    foreach ($captured as $call) {
        expect($call['sig'])->toBe(hash_hmac('sha256', "{$call['ts']}\n{$call['method']}\n{$call['path']}\n{$call['body']}", (string) config('services.sfu.secret')));
    }

    file_put_contents(getenv('AUDIT_HTTP_OUT') ?: '/dev/null', json_encode($captured));
});

it('DISCARD C4: the room-code token refuses a 26-char code (a channel ULID) and uppercase codes', function (): void {
    $user = User::factory()->create();

    $this->actingAs($user, 'sanctum')->postJson('/api/rooms/'.mb_strtolower((string) Str::ulid()).'/token')->assertNotFound();
    $this->actingAs($user, 'sanctum')->postJson('/api/rooms/ABCDEFGH/token')->assertNotFound();
});

it('DISCARD C5: a token issued for one user_limit seat is not blocked by ghost channel_accesses that never closed', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $voice->update(['user_limit' => 1]);
    fakeSfu($voice->id);

    foreach (range(1, 5) as $index) {
        ChannelAccess::query()->create(['channel_id' => $voice->id, 'user_id' => $owner->id, 'ip' => '10.0.0.'.$index, 'joined_at' => now()->subMinutes($index)]);
    }

    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk();
});

it('DISCARD C6: moving needs MOVE_MEMBERS on BOTH channels and CONNECT on the destination, and hierarchy', function (): void {
    $owner = User::factory()->create();
    $mod = User::factory()->create();
    $peer = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $mod);
    joinServer($server, $peer);
    joinServer($server, $member);
    giveRole($server, $mod, PermissionEnum::MoveMembers->value, 5);
    giveRole($server, $peer, 0, 5);
    $origin = $server->channels()->where('type', 'voice')->firstOrFail();
    $dest = $server->createChannel($owner, 'B', ChannelTypeEnum::Voice, null, null);
    auditPresence([$origin->id => [$member, $peer]]);

    $dest->overwrites()->create(['target_type' => 'member', 'target_id' => $mod->id, 'allow' => 0, 'deny' => PermissionEnum::MoveMembers->value]);
    $this->actingAs($mod, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $dest->id])->assertForbidden();
    $dest->overwrites()->delete();

    $origin->overwrites()->create(['target_type' => 'member', 'target_id' => $mod->id, 'allow' => 0, 'deny' => PermissionEnum::MoveMembers->value]);
    $this->actingAs($mod, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $dest->id])->assertForbidden();
    $origin->overwrites()->delete();

    $this->actingAs($mod, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$peer->id}", ['channel_id' => $dest->id])->assertForbidden();
    $this->actingAs($mod, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$owner->id}", ['channel_id' => $dest->id])->assertForbidden();

    $other = Server::createFor($owner, 'Outro');
    $foreign = $other->channels()->where('type', 'voice')->firstOrFail();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $foreign->id])->assertUnprocessable();

    $this->actingAs($mod, 'sanctum')->patchJson("/api/channels/{$origin->id}/voice/members/{$member->id}", ['channel_id' => $dest->id])->assertNoContent();
});
