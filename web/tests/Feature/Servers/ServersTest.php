<?php

declare(strict_types=1);

use App\Enums\PermissionEnum;
use App\Events\MemberRemoved;
use App\Events\ServerUpdated;
use App\Models\ChannelAccess;
use App\Models\Server;
use App\Models\ServerMember;
use App\Models\User;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\Http;
use OwenIt\Auditing\Models\Audit;

it('cria o servidor com @everyone, um canal de texto, um de voz e o dono dentro', function (): void {
    $owner = User::factory()->create();

    $this->actingAs($owner, 'sanctum')->postJson('/api/servers', ['name' => 'Meu servidor'])
        ->assertCreated()
        ->assertJsonPath('data.name', 'Meu servidor')
        ->assertJsonPath('data.owner_id', $owner->id);

    $server = Server::query()->firstOrFail();

    expect($server->roles()->where('is_everyone', true)->firstOrFail()->permissions)->toBe(PermissionEnum::everyoneDefault())
        ->and($server->channels()->where('type', 'text')->where('name', 'geral')->exists())->toBeTrue()
        ->and($server->channels()->where('type', 'voice')->where('name', 'Geral')->exists())->toBeTrue()
        ->and($server->members()->where('user_id', $owner->id)->exists())->toBeTrue()
        ->and($server->channels()->firstOrFail()->id)->toMatch('/^[a-z0-9]{26}$/');
});

it('a árvore mostra os canais, os cargos, os membros e o que eu posso fazer', function (): void {
    fakeSfu();
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');

    $this->actingAs($owner, 'sanctum')->getJson("/api/servers/{$server->id}")
        ->assertOk()
        ->assertJsonPath('data.me.user_id', $owner->id)
        ->assertJsonPath('data.me.permissions', PermissionEnum::all())
        ->assertJsonPath('data.invite_code', $server->invite_code)
        ->assertJsonCount(2, 'data.channels')
        ->assertJsonCount(1, 'data.roles')
        ->assertJsonCount(1, 'data.members')
        ->assertJsonPath('data.members.0.is_owner', true)
        ->assertJsonPath('data.roles.0.is_everyone', true);

    $this->actingAs($owner, 'sanctum')->getJson('/api/servers')->assertOk()->assertJsonCount(1, 'data');
});

it('a lista de servidores vai do último acesso à voz para o mais antigo, e o nunca acessado fica no fim pela data de entrada', function (): void {
    $this->freezeSecond();
    $user = User::factory()->create();
    $owner = User::factory()->create();
    $accessedDaysAgo = Server::createFor($owner, 'Acessado há dias');
    $accessedNow = Server::createFor($owner, 'Acessado agora');
    $joinedDaysAgo = Server::createFor($owner, 'Entrei há mais tempo');
    joinServer($accessedDaysAgo, $user)->update(['joined_at' => now()->subDays(10)]);
    joinServer($accessedNow, $user)->update(['joined_at' => now()->subDays(20)]);
    joinServer($joinedDaysAgo, $user)->update(['joined_at' => now()->subDays(8)]);

    $joinedNow = Server::createFor($user, 'Criei agora');

    $voiceOf = fn (Server $server): string => $server->channels()->where('type', 'voice')->firstOrFail()->id;

    ChannelAccess::query()->create(['channel_id' => $voiceOf($accessedDaysAgo), 'user_id' => $user->id, 'ip' => '10.0.0.1', 'joined_at' => now()->subDays(3)]);
    ChannelAccess::query()->create(['channel_id' => $voiceOf($accessedNow), 'user_id' => $user->id, 'ip' => '10.0.0.1', 'joined_at' => now()->subDays(5)]);
    ChannelAccess::query()->create(['channel_id' => $voiceOf($accessedNow), 'user_id' => $user->id, 'ip' => '10.0.0.1', 'joined_at' => now()->subHour()]);
    ChannelAccess::query()->create(['channel_id' => $voiceOf($joinedDaysAgo), 'user_id' => $owner->id, 'ip' => '10.0.0.1', 'joined_at' => now()]);

    $response = $this->actingAs($user, 'sanctum')->getJson('/api/servers')->assertOk();

    expect($response->json('data.*.id'))->toBe([$accessedNow->id, $accessedDaysAgo->id, $joinedNow->id, $joinedDaysAgo->id])
        ->and($response->json('data.0.last_accessed_at'))->toBe(now()->subHour()->toIso8601String())
        ->and($response->json('data.1.last_accessed_at'))->toBe(now()->subDays(3)->toIso8601String())
        ->and($response->json('data.2.last_accessed_at'))->toBeNull()
        ->and($response->json('data.3.last_accessed_at'))->toBeNull();
});

it('entra pelo convite, e quem foi banido não entra', function (): void {
    fakeSfu();
    $owner = User::factory()->create();
    $guest = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');

    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")
        ->assertOk()
        ->assertJsonPath('data.id', $server->id);

    expect($server->members()->where('user_id', $guest->id)->exists())->toBeTrue();

    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$guest->id}", ['reason' => 'spam'])
        ->assertCreated()
        ->assertJsonPath('data.user_id', $guest->id)
        ->assertJsonPath('data.reason', 'spam');

    expect($server->members()->where('user_id', $guest->id)->exists())->toBeFalse();

    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")->assertForbidden();
    $this->actingAs($guest, 'sanctum')->postJson('/api/invites/nao-existe')->assertNotFound();

    $this->actingAs($owner, 'sanctum')->getJson("/api/servers/{$server->id}/bans")->assertOk()->assertJsonCount(1, 'data');
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/servers/{$server->id}/bans/{$guest->id}")->assertNoContent();
    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")->assertOk();
});

it('gerar convite novo exige CREATE_INVITE', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $before = $server->invite_code;

    $this->actingAs($member, 'sanctum')->postJson("/api/servers/{$server->id}/invite")->assertOk();

    $server->everyoneRole()->update(['permissions' => PermissionEnum::everyoneDefault() & ~PermissionEnum::CreateInvite->value]);

    $this->actingAs($member, 'sanctum')->postJson("/api/servers/{$server->id}/invite")->assertForbidden();
    $this->actingAs($member, 'sanctum')->getJson("/api/servers/{$server->id}")->assertOk()->assertJsonPath('data.invite_code', null);

    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/invite")->assertOk();

    expect($server->refresh()->invite_code)->not->toBe($before);
});

it('a hierarquia recusa expulsar quem tem cargo igual ou acima', function (): void {
    fakeSfu();
    Event::fake([MemberRemoved::class, ServerUpdated::class]);

    $owner = User::factory()->create();
    $moderator = User::factory()->create();
    $peer = User::factory()->create();
    $newbie = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $moderator);
    joinServer($server, $peer);
    joinServer($server, $newbie);
    giveRole($server, $moderator, PermissionEnum::KickMembers->value, 2);
    giveRole($server, $peer, PermissionEnum::KickMembers->value, 2);

    $this->actingAs($moderator, 'sanctum')->deleteJson("/api/servers/{$server->id}/members/{$peer->id}")->assertForbidden();
    $this->actingAs($moderator, 'sanctum')->deleteJson("/api/servers/{$server->id}/members/{$owner->id}")->assertForbidden();
    $this->actingAs($newbie, 'sanctum')->deleteJson("/api/servers/{$server->id}/members/{$moderator->id}")->assertForbidden();
    $this->actingAs($moderator, 'sanctum')->deleteJson("/api/servers/{$server->id}/members/{$newbie->id}")->assertNoContent();

    expect($server->members()->where('user_id', $newbie->id)->exists())->toBeFalse();

    Event::assertDispatched(MemberRemoved::class, fn (MemberRemoved $event): bool => $event->userId === $newbie->id && $event->reason === 'kicked');
    Event::assertDispatched(ServerUpdated::class);
});

it('cargo só é criado abaixo do meu, e ninguém dá o que não tem', function (): void {
    $owner = User::factory()->create();
    $manager = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $manager);
    giveRole($server, $manager, PermissionEnum::ManageRoles->value, 5);

    $this->actingAs($manager, 'sanctum')->postJson("/api/servers/{$server->id}/roles", ['name' => 'Ajudante', 'permissions' => PermissionEnum::ManageRoles->value])
        ->assertCreated()
        ->assertJsonPath('data.position', 4);

    $this->actingAs($manager, 'sanctum')->postJson("/api/servers/{$server->id}/roles", ['name' => 'Chefe', 'permissions' => PermissionEnum::Administrator->value])
        ->assertForbidden();

    $everyone = $server->everyoneRole();

    $this->actingAs($manager, 'sanctum')->patchJson("/api/roles/{$everyone->id}", ['name' => 'todos'])->assertOk()->assertJsonPath('data.name', '@everyone');
    $this->actingAs($manager, 'sanctum')->deleteJson("/api/roles/{$everyone->id}")->assertUnprocessable();

    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/roles", ['name' => 'Topo', 'permissions' => PermissionEnum::Administrator->value])
        ->assertCreated()
        ->assertJsonPath('data.position', 6);
});

it('canal escondido some da árvore e as mensagens dele dão 403', function (): void {
    fakeSfu();
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);

    $secret = $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/channels", ['name' => 'segredo', 'type' => 'text'])
        ->assertCreated()
        ->json('data.id');

    $this->actingAs($owner, 'sanctum')->putJson("/api/channels/{$secret}/overwrites/role/{$server->everyoneRole()->id}", ['allow' => 0, 'deny' => PermissionEnum::ViewChannel->value])
        ->assertSuccessful()
        ->assertJsonPath('data.deny', PermissionEnum::ViewChannel->value);

    $tree = $this->actingAs($member, 'sanctum')->getJson("/api/servers/{$server->id}")->assertOk()->json('data');

    expect(collect($tree['channels'])->pluck('id')->all())->not->toContain($secret)
        ->and($tree['channels'][0]['overwrites'])->toBe([]);

    $this->actingAs($member, 'sanctum')->getJson("/api/channels/{$secret}/messages")->assertForbidden();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$secret}/messages", ['body' => 'oi'])->assertForbidden();

    $this->actingAs($owner, 'sanctum')->getJson("/api/servers/{$server->id}")->assertOk()->assertJsonCount(3, 'data.channels');
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/channels/{$secret}/overwrites/role/{$server->everyoneRole()->id}")->assertNoContent();
    $this->actingAs($member, 'sanctum')->getJson("/api/channels/{$secret}/messages")->assertOk();
});

it('o último canal de texto não pode ser apagado, e canal exige MANAGE_CHANNELS', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $text = $server->channels()->where('type', 'text')->firstOrFail();

    $this->actingAs($member, 'sanctum')->postJson("/api/servers/{$server->id}/channels", ['name' => 'x', 'type' => 'text'])->assertForbidden();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/channels/{$text->id}")->assertUnprocessable();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$text->id}", ['topic' => 'assunto'])->assertOk()->assertJsonPath('data.topic', 'assunto');
});

it('quem não é dono não apaga nem renomeia sem MANAGE_SERVER, e o dono não sai', function (): void {
    fakeSfu();
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);

    $this->actingAs($member, 'sanctum')->patchJson("/api/servers/{$server->id}", ['name' => 'Outra'])->assertForbidden();
    $this->actingAs($member, 'sanctum')->deleteJson("/api/servers/{$server->id}")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/leave")->assertForbidden();
    $this->actingAs($member, 'sanctum')->postJson("/api/servers/{$server->id}/leave")->assertNoContent();
    $this->actingAs($member, 'sanctum')->getJson("/api/servers/{$server->id}")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}", ['name' => 'Outra'])->assertOk()->assertJsonPath('data.name', 'Outra');
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/servers/{$server->id}")->assertNoContent();

    $this->assertDatabaseCount('servers', 0);
    $this->assertDatabaseCount('channels', 0);
    $this->assertDatabaseCount('server_roles', 0);
});

it('mutar no servidor exige MUTE_MEMBERS e avisa o SFU quando a pessoa está em voz', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();

    fakeSfu($voice->id, $member);

    $this->actingAs($member, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$owner->id}", ['server_mute' => true])->assertForbidden();
    $this->actingAs($member, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['nickname' => 'eu'])->assertOk()->assertJsonPath('data.nickname', 'eu');
    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['server_mute' => true])->assertOk()->assertJsonPath('data.server_mute', true);

    Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$voice->id}/mute") && $request['muted'] === true && $request->hasHeader('X-Unkvoid-Signature'));

    $this->actingAs($owner, 'sanctum')->getJson("/api/servers/{$server->id}")
        ->assertOk()
        ->assertJsonPath("data.voice.{$voice->id}.0.user_id", $member->id)
        ->assertJsonPath("data.voice.{$voice->id}.0.sources.0", 'mic');
});

it('o GET /api/config entrega o SFU e o Reverb sem login', function (): void {
    $this->getJson('/api/config')
        ->assertOk()
        ->assertJsonPath('data.sfu', 'ws://127.0.0.1:3000/sfu')
        ->assertJsonStructure(['data' => ['sfu', 'reverb' => ['host', 'port', 'key', 'scheme']]]);
});

it('tudo de servidor exige token', function (): void {
    $this->getJson('/api/servers')->assertUnauthorized();
    $this->postJson('/api/servers', ['name' => 'x'])->assertUnauthorized();
});

it('o Reverb autoriza pelo token do Sanctum: canal só com VIEW_CHANNEL, servidor só membro, usuário só o próprio', function (): void {
    // Os canais são registrados no broadcaster padrão quando o app sobe (null no teste):
    // trocando para o reverb, o arquivo precisa ser lido de novo.
    config(['broadcasting.default' => 'reverb']);
    require base_path('routes/channels.php');
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $stranger = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $text = $server->channels()->where('type', 'text')->firstOrFail();

    $this->postJson('/broadcasting/auth', ['channel_name' => "private-channel.{$text->id}", 'socket_id' => '1.1'])->assertUnauthorized();
    $this->actingAs($stranger, 'sanctum')->postJson('/broadcasting/auth', ['channel_name' => "private-channel.{$text->id}", 'socket_id' => '1.1'])->assertForbidden();
    $this->actingAs($member, 'sanctum')->postJson('/broadcasting/auth', ['channel_name' => "private-channel.{$text->id}", 'socket_id' => '1.1'])->assertOk();
    $this->actingAs($member, 'sanctum')->postJson('/broadcasting/auth', ['channel_name' => "presence-server.{$server->id}", 'socket_id' => '1.1'])->assertOk()->assertJsonStructure(['channel_data']);
    $this->actingAs($member, 'sanctum')->postJson('/broadcasting/auth', ['channel_name' => "private-user.{$member->id}", 'socket_id' => '1.1'])->assertOk();
    $this->actingAs($member, 'sanctum')->postJson('/broadcasting/auth', ['channel_name' => "private-user.{$owner->id}", 'socket_id' => '1.1'])->assertForbidden();

    $text->overwrites()->create(['target_type' => 'role', 'target_id' => $server->everyoneRole()->id, 'allow' => 0, 'deny' => PermissionEnum::ViewChannel->value]);

    $this->actingAs($member, 'sanctum')->postJson('/broadcasting/auth', ['channel_name' => "private-channel.{$text->id}", 'socket_id' => '1.1'])->assertForbidden();
});

it('membro de A não toca em nada de B', function (): void {
    fakeSfu();
    $ownerA = User::factory()->create();
    $ownerB = User::factory()->create();
    $serverA = Server::createFor($ownerA, 'A');
    $serverB = Server::createFor($ownerB, 'B');
    $channelA = $serverA->channels()->where('type', 'text')->firstOrFail();
    $channelB = $serverB->channels()->where('type', 'text')->firstOrFail();
    $roleB = $serverB->roles()->create(['name' => 'De B', 'position' => 1, 'permissions' => 0]);

    $this->actingAs($ownerA, 'sanctum')->getJson("/api/servers/{$serverB->id}")->assertForbidden();
    $this->actingAs($ownerA, 'sanctum')->patchJson("/api/channels/{$channelB->id}", ['name' => 'invadido'])->assertForbidden();
    $this->actingAs($ownerA, 'sanctum')->putJson("/api/channels/{$channelB->id}/overwrites/role/{$roleB->id}", ['allow' => 0, 'deny' => 0])->assertForbidden();
    $this->actingAs($ownerA, 'sanctum')->putJson("/api/channels/{$channelA->id}/overwrites/role/{$roleB->id}", ['allow' => 0, 'deny' => 0])->assertUnprocessable();
    $this->actingAs($ownerA, 'sanctum')->patchJson("/api/roles/{$roleB->id}", ['name' => 'invadido'])->assertForbidden();
    $this->actingAs($ownerA, 'sanctum')->deleteJson("/api/roles/{$roleB->id}")->assertForbidden();
    $this->actingAs($ownerA, 'sanctum')->deleteJson("/api/servers/{$serverB->id}/members/{$ownerB->id}")->assertForbidden();
    $this->actingAs($ownerA, 'sanctum')->postJson("/api/channels/{$channelB->id}/messages", ['body' => 'oi'])->assertForbidden();

    expect($channelB->refresh()->name)->toBe('geral')
        ->and($roleB->refresh()->name)->toBe('De B');
});

it('editar cargo não sobe acima do meu, não dá o que não tenho, e a posição começa em 1', function (): void {
    $owner = User::factory()->create();
    $manager = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $manager);
    giveRole($server, $manager, PermissionEnum::ManageRoles->value, 5);
    $below = $server->roles()->create(['name' => 'Abaixo', 'position' => 2, 'permissions' => 0]);
    $above = $server->roles()->create(['name' => 'Acima', 'position' => 7, 'permissions' => 0]);

    $this->actingAs($manager, 'sanctum')->patchJson("/api/roles/{$below->id}", ['position' => 5])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/roles/{$below->id}", ['position' => 0])->assertUnprocessable();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/roles/{$below->id}", ['permissions' => PermissionEnum::Administrator->value])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/roles/{$above->id}", ['name' => 'Meu'])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->deleteJson("/api/roles/{$above->id}")->assertForbidden();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/roles/{$below->id}", ['position' => 4, 'permissions' => PermissionEnum::ManageRoles->value])->assertOk()->assertJsonPath('data.position', 4);
    $this->actingAs($manager, 'sanctum')->deleteJson("/api/roles/{$below->id}")->assertNoContent();

    $low = User::factory()->create();
    joinServer($server, $low);
    giveRole($server, $low, PermissionEnum::ManageRoles->value, 1);

    $this->actingAs($low, 'sanctum')->postJson("/api/servers/{$server->id}/roles", ['name' => 'Raso', 'permissions' => 0])->assertCreated()->assertJsonPath('data.position', 1);
});

it('dar cargo exige MANAGE_ROLES, cargo abaixo do meu e permissões que eu tenho, e fica na auditoria', function (): void {
    config(['audit.console' => true]);
    $owner = User::factory()->create();
    $manager = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $manager);
    joinServer($server, $member);
    giveRole($server, $manager, PermissionEnum::ManageRoles->value, 5);
    $helper = $server->roles()->create(['name' => 'Ajudante', 'position' => 2, 'permissions' => PermissionEnum::SendMessages->value]);
    $boss = $server->roles()->create(['name' => 'Chefe', 'position' => 6, 'permissions' => 0]);
    $admin = $server->roles()->create(['name' => 'Admin', 'position' => 1, 'permissions' => PermissionEnum::Administrator->value]);

    $this->actingAs($member, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['role_ids' => [$helper->id]])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['role_ids' => [$boss->id]])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['role_ids' => [$admin->id]])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$manager->id}", ['role_ids' => [$admin->id]])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['role_ids' => [$helper->id, $server->everyoneRole()->id]])->assertOk()->assertJsonPath('data.role_ids', [$helper->id]);

    expect(Audit::query()->where('event', 'sync')->where('auditable_type', ServerMember::class)->exists())->toBeTrue();

    $this->actingAs($manager, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['role_ids' => []])->assertOk()->assertJsonPath('data.role_ids', []);
});

it('banir segue a hierarquia, nunca pega o dono, derruba da voz e avisa quem saiu', function (): void {
    Event::fake([MemberRemoved::class, ServerUpdated::class]);
    $owner = User::factory()->create();
    $moderator = User::factory()->create();
    $peer = User::factory()->create();
    $plain = User::factory()->create();
    $newbie = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $moderator);
    joinServer($server, $peer);
    joinServer($server, $plain);
    joinServer($server, $newbie);
    giveRole($server, $moderator, PermissionEnum::BanMembers->value, 2);
    giveRole($server, $peer, PermissionEnum::BanMembers->value, 2);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id, $newbie);

    $this->actingAs($moderator, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$peer->id}")->assertForbidden();
    $this->actingAs($moderator, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$owner->id}")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$owner->id}")->assertForbidden();
    $this->actingAs($plain, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$newbie->id}")->assertForbidden();
    $this->actingAs($moderator, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$newbie->id}")->assertCreated();

    Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$voice->id}/kick") && $request['userId'] === "user:{$newbie->id}");
    Event::assertDispatched(MemberRemoved::class, fn (MemberRemoved $event): bool => $event->userId === $newbie->id && $event->reason === 'banned' && $event->broadcastOn()->name === "private-user.{$newbie->id}");

    $this->actingAs($plain, 'sanctum')->getJson("/api/servers/{$server->id}/bans")->assertForbidden();
    $this->actingAs($plain, 'sanctum')->deleteJson("/api/servers/{$server->id}/bans/{$newbie->id}")->assertForbidden();
    $this->actingAs($plain, 'sanctum')->getJson("/api/servers/{$server->id}")->assertOk()->assertJsonPath('data.bans', []);
    $this->actingAs($moderator, 'sanctum')->getJson("/api/servers/{$server->id}/bans")->assertOk()->assertJsonCount(1, 'data');
});

it('expulsar e banir continuam valendo com o SFU fora do ar', function (): void {
    Http::fake(['*' => Http::response('', 500)]);
    $owner = User::factory()->create();
    $first = User::factory()->create();
    $second = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $first);
    joinServer($server, $second);

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/servers/{$server->id}/members/{$first->id}")->assertNoContent();
    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$second->id}")->assertCreated();

    expect($server->members()->count())->toBe(1);
});

it('sobrescrita só com os bits que eu tenho no canal, e só em cargo abaixo do meu', function (): void {
    $owner = User::factory()->create();
    $manager = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $manager);
    giveRole($server, $manager, PermissionEnum::ManageRoles->value, 3);
    $text = $server->channels()->where('type', 'text')->firstOrFail();
    $below = $server->roles()->create(['name' => 'Abaixo', 'position' => 1, 'permissions' => 0]);
    $above = $server->roles()->create(['name' => 'Acima', 'position' => 5, 'permissions' => 0]);

    $this->actingAs($manager, 'sanctum')->putJson("/api/channels/{$text->id}/overwrites/role/{$below->id}", ['allow' => PermissionEnum::Administrator->value, 'deny' => 0])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->putJson("/api/channels/{$text->id}/overwrites/role/{$below->id}", ['allow' => 0, 'deny' => PermissionEnum::KickMembers->value])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->putJson("/api/channels/{$text->id}/overwrites/role/{$above->id}", ['allow' => 0, 'deny' => PermissionEnum::SendMessages->value])->assertForbidden();
    $this->actingAs($manager, 'sanctum')->putJson("/api/channels/{$text->id}/overwrites/role/999", ['allow' => 0, 'deny' => 0])->assertUnprocessable();
    $this->actingAs($manager, 'sanctum')->putJson("/api/channels/{$text->id}/overwrites/member/".User::factory()->create()->id, ['allow' => 0, 'deny' => 0])->assertUnprocessable();
    $this->actingAs($manager, 'sanctum')->putJson("/api/channels/{$text->id}/overwrites/role/{$below->id}", ['allow' => 0, 'deny' => PermissionEnum::SendMessages->value])->assertSuccessful();
    $this->actingAs($manager, 'sanctum')->putJson("/api/channels/{$text->id}/overwrites/member/{$manager->id}", ['allow' => PermissionEnum::SendMessages->value, 'deny' => 0])->assertSuccessful();
    $this->actingAs($manager, 'sanctum')->deleteJson("/api/channels/{$text->id}/overwrites/role/{$above->id}")->assertForbidden();
    $this->actingAs($manager, 'sanctum')->deleteJson("/api/channels/{$text->id}/overwrites/role/{$below->id}")->assertNoContent();
    $this->actingAs($owner, 'sanctum')->putJson("/api/channels/{$text->id}/overwrites/member/{$owner->id}", ['allow' => 0, 'deny' => PermissionEnum::ViewChannel->value])->assertSuccessful();

    expect($text->overwrites()->count())->toBe(2);
});

it('apagar o cargo, sair, expulsar e banir levam as sobrescritas junto', function (): void {
    fakeSfu();
    $owner = User::factory()->create();
    $leaver = User::factory()->create();
    $kicked = User::factory()->create();
    $banned = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $leaver);
    joinServer($server, $kicked);
    joinServer($server, $banned);
    $role = $server->roles()->create(['name' => 'Cargo', 'position' => 1, 'permissions' => 0]);
    $text = $server->channels()->where('type', 'text')->firstOrFail();
    $text->overwrites()->create(['target_type' => 'role', 'target_id' => $role->id, 'allow' => 0, 'deny' => 0]);

    foreach ([$leaver, $kicked, $banned] as $user) {
        $text->overwrites()->create(['target_type' => 'member', 'target_id' => $user->id, 'allow' => 0, 'deny' => 0]);
    }

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/roles/{$role->id}")->assertNoContent();
    $this->actingAs($leaver, 'sanctum')->postJson("/api/servers/{$server->id}/leave")->assertNoContent();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/servers/{$server->id}/members/{$kicked->id}")->assertNoContent();
    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/bans/{$banned->id}")->assertCreated();

    $this->assertDatabaseCount('channel_overwrites', 0);
});

it('sair do servidor derruba da voz', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    fakeSfu($voice->id, $member);

    $this->actingAs($member, 'sanctum')->postJson("/api/servers/{$server->id}/leave")->assertNoContent();

    Http::assertSent(fn ($request): bool => str_ends_with((string) $request->url(), "/rooms/{$voice->id}/kick") && $request['userId'] === "user:{$member->id}");
});

it('user_limit só existe em canal de voz', function (): void {
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $text = $server->channels()->where('type', 'text')->firstOrFail();
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();

    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/channels", ['name' => 'x', 'type' => 'text', 'user_limit' => 5])->assertUnprocessable();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$text->id}", ['user_limit' => 5])->assertUnprocessable();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$voice->id}", ['user_limit' => 5])->assertOk()->assertJsonPath('data.user_limit', 5);
});
