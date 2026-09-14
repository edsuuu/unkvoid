<?php

declare(strict_types=1);

use App\Livewire\Admin\Audit\Index;
use App\Models\ChannelAccess;
use App\Models\ChannelAudit;
use App\Models\Server;
use App\Models\User;
use Database\Seeders\Seeder001Roles;
use Illuminate\Support\Facades\Http;
use Livewire\Livewire;

it('a página de auditoria lista os acessos e filtra por servidor, e-mail e data', function (): void {
    Http::fake();
    $this->seed(Seeder001Roles::class);
    $admin = User::factory()->create(['email' => config('unkvoid.admin_email')]);
    $alice = User::factory()->create(['email' => 'alice@unkvoid.test']);
    $bob = User::factory()->create(['email' => 'bob@unkvoid.test']);
    $casa = Server::createFor($alice, 'Casa');
    $trabalho = Server::createFor($bob, 'Trabalho');
    $voiceCasa = $casa->channels()->where('type', 'voice')->firstOrFail();
    $voiceTrabalho = $trabalho->channels()->where('type', 'voice')->firstOrFail();

    ChannelAccess::query()->create(['channel_id' => $voiceCasa->id, 'user_id' => $alice->id, 'ip' => '10.0.0.1', 'sfu_ip' => '203.0.113.1', 'user_agent' => 'Unkvoid/1', 'joined_at' => '2026-09-01 12:00:00', 'left_at' => '2026-09-01 12:30:00']);
    ChannelAccess::query()->create(['channel_id' => $voiceTrabalho->id, 'user_id' => $bob->id, 'ip' => '10.0.0.2', 'joined_at' => '2026-09-10 09:00:00']);

    $this->actingAs($admin)->get(route('admin.audit'))->assertOk()->assertSee('Auditoria');
    $this->actingAs($alice)->get(route('admin.audit'))->assertForbidden();

    Livewire::actingAs($admin)->test(Index::class)
        ->assertSee('alice@unkvoid.test')
        ->assertSee('bob@unkvoid.test')
        ->assertSee('203.0.113.1')
        ->assertSee('em chamada')
        ->set('serverId', (string) $casa->id)
        ->assertSee('alice@unkvoid.test')
        ->assertDontSee('bob@unkvoid.test')
        ->set('serverId', '')
        ->set('email', 'bob@')
        ->assertDontSee('alice@unkvoid.test')
        ->assertSee('bob@unkvoid.test')
        ->set('email', '')
        ->set('from', '2026-09-05')
        ->assertDontSee('alice@unkvoid.test')
        ->assertSee('bob@unkvoid.test')
        ->set('from', '')
        ->set('until', '2026-09-05')
        ->assertSee('alice@unkvoid.test')
        ->assertDontSee('bob@unkvoid.test');
});

it('a aba de alterações mostra o que mudou nos servidores', function (): void {
    Http::fake();
    config(['audit.console' => true]);
    $this->seed(Seeder001Roles::class);
    $admin = User::factory()->create(['email' => config('unkvoid.admin_email')]);
    $owner = User::factory()->create(['email' => 'dona@unkvoid.test']);
    $server = Server::createFor($owner, 'Casa');

    $this->actingAs($owner, 'sanctum')->patchJson("/api/servers/{$server->id}", ['name' => 'Casa nova'])->assertOk();

    Livewire::actingAs($admin)->test(Index::class)
        ->call('showTab', 'changes')
        ->assertSee('Server #'.$server->id)
        ->assertSee('updated')
        ->assertSee("name: 'Casa nova'")
        ->assertSee($owner->name)
        ->set('email', 'ninguem@')
        ->assertDontSee('Casa nova');
});

it('a página de auditoria carrega mais registros ao clicar no botão', function (): void {
    Http::fake();
    $this->seed(Seeder001Roles::class);
    $admin = User::factory()->create(['email' => config('unkvoid.admin_email')]);
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();

    for ($index = 1; $index <= 201; $index++) {
        ChannelAccess::query()->create(['channel_id' => $voice->id, 'user_id' => $owner->id, 'ip' => '10.0.0.1', 'joined_at' => now()->subMinutes($index)]);
    }

    Livewire::actingAs($admin)->test(Index::class)
        ->assertSee('Carregar mais')
        ->call('loadMore')
        ->assertSet('limit', 400)
        ->assertDontSee('Carregar mais');
});

it('o histórico do canal guarda criação, renomeação e exclusão, e aparece na aba de alterações', function (): void {
    Http::fake();
    $this->seed(Seeder001Roles::class);
    $admin = User::factory()->create(['email' => config('unkvoid.admin_email')]);
    $owner = User::factory()->create(['email' => 'dona@unkvoid.test']);
    $server = Server::createFor($owner, 'Casa');

    // Criar o servidor já semeia dois canais: é aí que o id ULID entra no histórico.
    expect(ChannelAudit::query()->where('event', 'created')->count())->toBe(2);

    $extra = $this->actingAs($owner, 'sanctum')
        ->postJson("/api/servers/{$server->id}/channels", ['name' => 'links', 'type' => 'text'])
        ->assertCreated()
        ->json('data.id');

    $this->actingAs($owner, 'sanctum')->patchJson("/api/channels/{$extra}", ['name' => 'avisos'])->assertOk();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/channels/{$extra}")->assertNoContent();

    $renamed = ChannelAudit::query()->where('event', 'updated')->where('channel_id', $extra)->firstOrFail();

    expect($renamed->old_values['name'])->toBe('links')
        ->and($renamed->new_values['name'])->toBe('avisos')
        ->and($renamed->user_id)->toBe($owner->id)
        ->and(ChannelAudit::query()->where('event', 'deleted')->where('channel_id', $extra)->firstOrFail()->old_values['name'])->toBe('avisos');

    Livewire::actingAs($admin)->test(Index::class)
        ->call('showTab', 'changes')
        ->assertSee('Channel #'.$extra)
        ->assertSee('deleted')
        ->assertSee($owner->name);
});
