<?php

declare(strict_types=1);

use App\Livewire\Admin\Errors\Index;
use App\Models\ErrorReport;
use App\Models\User;
use Database\Seeders\Seeder001Roles;
use Livewire\Livewire;

function crashLog(string $when = '2026-09-11T16:01:53.425615Z'): string
{
    return implode("\n", [
        '--- unkvoid 0.0.22 em windows ---',
        $when.' INFO broadcast: ligando captura e encoder',
        $when.' panic crates/capture/src/windows.rs:214:9 :: called `Option::unwrap()` on a `None` value',
    ]);
}

it('guarda o relatório que o app manda e devolve o erro agrupado', function (): void {
    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'windows', 'log' => crashLog()])
        ->assertCreated()
        ->assertJsonPath('data.platform', 'windows')
        ->assertJsonPath('data.occurrences', 1);

    expect(ErrorReport::query()->firstOrFail()->signature)->toContain('windows.rs');
});

it('a mesma falha em computadores diferentes vira uma linha só, com o contador subindo', function (): void {
    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'windows', 'log' => crashLog()])->assertCreated();
    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'windows', 'log' => crashLog('2026-09-12T09:40:00.000000Z')])
        ->assertSuccessful()
        ->assertJsonPath('data.occurrences', 2);

    expect(ErrorReport::query()->count())->toBe(1);
});

it('erro de outra versão ou de outro sistema não se mistura', function (): void {
    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'windows', 'log' => crashLog()])->assertCreated();
    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'linux', 'log' => crashLog()])->assertCreated();
    $this->postJson('/api/errors', ['version' => '0.0.21', 'platform' => 'windows', 'log' => crashLog()])->assertCreated();

    expect(ErrorReport::query()->count())->toBe(3);
});

it('recusa o que não é relatório de erro', function (): void {
    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'windows'])->assertStatus(422);
    $this->postJson('/api/errors', ['version' => 'nightly', 'platform' => 'windows', 'log' => crashLog()])->assertStatus(422);
    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'haiku', 'log' => crashLog()])->assertStatus(422);
    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'windows', 'log' => str_repeat('a', 20_001)])->assertStatus(422);

    expect(ErrorReport::query()->count())->toBe(0);
});

it('o painel lista os erros, filtra por sistema e apaga', function (): void {
    $this->seed(Seeder001Roles::class);
    $admin = User::factory()->create(['email' => config('unkvoid.admin_email')]);

    $this->postJson('/api/errors', ['version' => '0.0.22', 'platform' => 'windows', 'log' => crashLog()])->assertCreated();

    $this->actingAs($admin)->get(route('admin.errors'))->assertOk()->assertSee('Erros dos apps instalados');

    Livewire::actingAs($admin)->test(Index::class)
        ->assertSee('windows.rs')
        ->set('platform', 'linux')
        ->assertDontSee('windows.rs')
        ->set('platform', 'windows')
        ->assertSee('windows.rs')
        ->call('remove', ErrorReport::query()->firstOrFail()->id);

    expect(ErrorReport::query()->count())->toBe(0);
});
