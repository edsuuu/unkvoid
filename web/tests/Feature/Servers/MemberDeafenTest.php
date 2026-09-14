<?php

declare(strict_types=1);

use App\Enums\PermissionEnum;
use App\Models\Server;
use App\Models\User;
use Illuminate\Support\Facades\Http;

it('ensurdecer no servidor exige DEAFEN_MEMBERS, respeita a hierarquia e nunca chama o SFU', function (): void {
    Http::fake();
    $owner = User::factory()->create();
    $target = User::factory()->create();
    $bystander = User::factory()->create();
    $lowMod = User::factory()->create();
    $bigBoss = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $target);
    joinServer($server, $bystander);
    joinServer($server, $lowMod);
    joinServer($server, $bigBoss);
    giveRole($server, $target, 0, 5);
    giveRole($server, $lowMod, PermissionEnum::DeafenMembers->value, 3);
    giveRole($server, $bigBoss, PermissionEnum::DeafenMembers->value, 10);

    $this->actingAs($bystander, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$target->id}", ['server_deaf' => true])->assertForbidden();
    $this->actingAs($lowMod, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$target->id}", ['server_deaf' => true])->assertForbidden();
    $this->actingAs($bigBoss, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$target->id}", ['server_deaf' => true])->assertOk()->assertJsonPath('data.server_deaf', true);
    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}/members/{$target->id}", ['server_deaf' => false])->assertOk()->assertJsonPath('data.server_deaf', false);

    Http::assertNothingSent();
});
