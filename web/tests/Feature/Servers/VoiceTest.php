<?php

declare(strict_types=1);

use App\Enums\PermissionEnum;
use App\Events\VoiceStateUpdated;
use App\Models\ChannelAccess;
use App\Models\GuestAccess;
use App\Models\Server;
use App\Models\User;
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

it('mutado no servidor não leva speak, e sem CONNECT não leva token', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member)->update(['server_mute' => true]);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $text = $server->channels()->where('type', 'text')->firstOrFail();
    fakeSfu($voice->id);

    $token = (string) $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/voice/token")->assertOk()->json('data.token');
    $claims = json_decode(base64_decode(strtr(explode('.', $token)[0], '-_', '+/'), true), true, 512, JSON_THROW_ON_ERROR);

    expect($claims['can'])->toBe(['stream', 'video']);

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

    Event::assertDispatched(VoiceStateUpdated::class, fn (VoiceStateUpdated $event): bool => $event->event === 'joined' && $event->userId === $owner->id && $event->broadcastOn()->name === "private-channel.{$voice->id}");

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
