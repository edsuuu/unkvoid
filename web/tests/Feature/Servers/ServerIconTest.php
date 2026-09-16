<?php

declare(strict_types=1);

use App\Enums\PermissionEnum;
use App\Models\Server;
use App\Models\User;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\Http;

beforeEach(function (): void {
    Http::fake();
});

it('o ícone sobe, troca apagando o antigo e some, sempre com MANAGE_SERVER', function (): void {
    $commands = fakeS3Client();
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);

    $this->actingAs($owner, 'sanctum')->getJson("/api/servers/{$server->id}")->assertOk()->assertJsonPath('data.icon_url', null);

    $this->actingAs($member, 'sanctum')
        ->postJson("/api/servers/{$server->id}/icon", ['icon' => UploadedFile::fake()->image('icone.jpg')])
        ->assertForbidden();

    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/servers/{$server->id}/icon", ['icon' => UploadedFile::fake()->image('icone.jpg')])
        ->assertOk()
        ->assertJsonPath('data.icon_url', fn (?string $url): bool => ! is_null($url));

    $first = $server->refresh()->icon_path;

    expect($first)->toStartWith("servers/{$server->id}/")
        ->and($commands->getArrayCopy())->toContain('PutObject')
        ->and($commands->getArrayCopy())->not->toContain('DeleteObject');

    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/servers/{$server->id}/icon", ['icon' => UploadedFile::fake()->image('outro.png')])
        ->assertOk();

    expect($server->refresh()->icon_path)->not->toBe($first)
        ->and($commands->getArrayCopy())->toContain('DeleteObject');

    $this->actingAs($owner, 'sanctum')->getJson('/api/servers')->assertOk()->assertJsonPath('data.0.icon_url', fn (?string $url): bool => ! is_null($url));
    $this->actingAs($owner, 'sanctum')->getJson("/api/servers/{$server->id}")->assertOk()->assertJsonPath('data.icon_url', fn (?string $url): bool => ! is_null($url));

    $this->actingAs($member, 'sanctum')->deleteJson("/api/servers/{$server->id}/icon")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/servers/{$server->id}/icon")->assertNoContent();

    expect($server->refresh()->icon_path)->toBeNull();

    $this->actingAs($owner, 'sanctum')->getJson("/api/servers/{$server->id}")->assertOk()->assertJsonPath('data.icon_url', null);
});

it('quem tem MANAGE_SERVER sem ser dono troca o ícone, e só imagem de até 2 MB entra', function (): void {
    fakeS3Client();
    $owner = User::factory()->create();
    $member = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    giveRole($server, $member, PermissionEnum::ManageServer->value);

    $this->actingAs($member, 'sanctum')
        ->postJson("/api/servers/{$server->id}/icon", ['icon' => UploadedFile::fake()->image('icone.webp')])
        ->assertOk();

    $this->actingAs($owner, 'sanctum')->postJson("/api/servers/{$server->id}/icon")->assertUnprocessable();
    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/servers/{$server->id}/icon", ['icon' => UploadedFile::fake()->create('livro.pdf', 10, 'application/pdf')])
        ->assertUnprocessable();
    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/servers/{$server->id}/icon", ['icon' => UploadedFile::fake()->create('enorme.jpg', 2100, 'image/jpeg')])
        ->assertUnprocessable();
});
