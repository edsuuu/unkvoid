<?php

declare(strict_types=1);

use App\Enums\PermissionEnum;
use App\Models\Server;
use App\Models\User;
use Illuminate\Support\Facades\Http;

beforeEach(function (): void {
    Http::fake();
    config(['audit.console' => true]);
});

it('o histórico do servidor junta as duas tabelas e conta em português o que houve', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}", ['name' => 'Casa nova'])->assertOk();

    $roleId = $this->actingAs($owner, 'sanctum')
        ->postJson("/api/servers/{$server->id}/roles", ['name' => 'Moderador', 'permissions' => PermissionEnum::ManageMessages->value])
        ->assertCreated()
        ->json('data.id');

    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$member->id}", ['role_ids' => [$roleId]])->assertOk();

    $extra = $this->actingAs($owner, 'sanctum')
        ->postJson("/api/servers/{$server->id}/channels", ['name' => 'links', 'type' => 'text'])
        ->assertCreated()
        ->json('data.id');

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/channels/{$extra}")->assertNoContent();

    $messageId = $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi'])->assertCreated()->json('data.id');
    $this->actingAs($member, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();

    $response = $this->actingAs($owner, 'sanctum')->getJson("/api/servers/{$server->id}/audits")->assertOk();
    $rows = $response->json('data');
    $summaries = array_column($rows, 'summary');

    expect($summaries)->toContain('criou o servidor Casa')
        ->and($summaries)->toContain('mudou o servidor Casa nova')
        ->and($summaries)->toContain('criou o cargo Moderador')
        ->and($summaries)->toContain('mudou os cargos de um membro')
        ->and($summaries)->toContain('criou o canal #links')
        ->and($summaries)->toContain('apagou o canal #links')
        ->and($summaries)->toContain('apagou uma mensagem')
        ->and(array_column($rows, 'type'))->toContain('Channel', 'Server', 'ServerRole', 'ServerMember', 'Message')
        ->and(array_column($rows, 'id'))->toHaveCount(count(array_unique(array_column($rows, 'id'))));

    $mine = array_values(array_filter($rows, fn (array $row): bool => $row['summary'] === 'apagou uma mensagem'));

    expect($mine[0]['actor']['id'])->toBe($member->id)
        ->and($mine[0]['actor']['name'])->toBe($member->name)
        ->and($mine[0]['at'])->not->toBeNull();
});

it('o histórico exige VIEW_AUDIT_LOG e não mistura servidor com servidor', function (): void {
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $stranger = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $other = Server::createFor($stranger, 'Trabalho');
    joinServer($server, $member);

    $this->actingAs($member, 'sanctum')->getJson("/api/servers/{$server->id}/audits")->assertForbidden();
    $this->actingAs($stranger, 'sanctum')->getJson("/api/servers/{$server->id}/audits")->assertForbidden();

    giveRole($server, $member, PermissionEnum::ViewAuditLog->value);

    $summaries = array_column($this->actingAs($member, 'sanctum')->getJson("/api/servers/{$server->id}/audits")->assertOk()->json('data'), 'summary');

    expect($summaries)->toContain('criou o servidor Casa')
        ->and($summaries)->not->toContain('criou o servidor Trabalho');

    $this->actingAs($stranger, 'sanctum')->getJson("/api/servers/{$other->id}/audits")->assertOk();
});
