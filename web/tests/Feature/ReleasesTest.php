<?php

declare(strict_types=1);

use App\Enums\ReleasePlatformEnum;
use App\Livewire\Admin\Releases\Index;
use App\Models\Release;
use App\Models\User;
use Database\Seeders\Seeder001Roles;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\Storage;
use Livewire\Livewire;

function signedHeaders(string $path, UploadedFile $file, ?string $secret = null): array
{
    $timestamp = (string) time();
    $hash = hash_file('sha256', $file->getRealPath());
    $signature = hash_hmac('sha256', implode("\n", [$timestamp, 'POST', $path, $hash]), $secret ?? config('unkvoid.release_secret'));

    return ['X-Unkvoid-Timestamp' => $timestamp, 'X-Unkvoid-Signature' => $signature];
}

beforeEach(function (): void {
    Storage::fake('s3');
    Storage::disk('s3')->buildTemporaryUrlsUsing(fn (string $path): string => 'https://s3.unkvoid.test/'.$path.'?assinada=1');
});

it('recusa publicar sem assinatura ou com assinatura errada', function (): void {
    $file = UploadedFile::fake()->create('Unkvoid_0.0.8_x64_pt-BR.msi', 100);

    $this->postJson('/api/releases', ['version' => '0.0.8', 'platform' => 'windows-x86_64-msi', 'file' => $file])->assertStatus(401);

    $this->postJson('/api/releases', ['version' => '0.0.8', 'platform' => 'windows-x86_64-msi', 'file' => $file], signedHeaders('/api/releases', $file, 'outro-segredo'))
        ->assertStatus(401);
});

it('publica pela API assinada, guarda no bucket e monta o latest.json com URL assinada', function (): void {
    $file = UploadedFile::fake()->create('Unkvoid_0.0.8_x64_pt-BR.msi', 100);

    $this->postJson('/api/releases', ['version' => '0.0.8', 'platform' => 'windows-x86_64-msi', 'file' => $file, 'signature' => 'assinatura-minisign'], signedHeaders('/api/releases', $file))
        ->assertCreated()
        ->assertJsonPath('data.platform', 'windows-x86_64-msi');

    Storage::disk('s3')->assertExists('releases/0.0.8/Unkvoid_0.0.8_x64_pt-BR.msi');

    $this->getJson('/downloads/latest.json')
        ->assertOk()
        ->assertJsonPath('version', '0.0.8')
        ->assertJsonPath('platforms.windows-x86_64-msi.signature', 'assinatura-minisign')
        ->assertJsonPath('platforms.windows-x86_64.signature', 'assinatura-minisign')
        ->assertJsonPath('platforms.windows-x86_64-msi.url', 'https://s3.unkvoid.test/releases/0.0.8/Unkvoid_0.0.8_x64_pt-BR.msi?assinada=1');

    $this->get('/downloads/windows-msi')->assertRedirect('https://s3.unkvoid.test/releases/0.0.8/Unkvoid_0.0.8_x64_pt-BR.msi?assinada=1');
    $this->get('/downloads/macos')->assertNotFound();
});

it('publicar a mesma versão de novo substitui em vez de duplicar', function (): void {
    $first = UploadedFile::fake()->create('Unkvoid_0.0.8_amd64.deb', 50);
    $second = UploadedFile::fake()->create('Unkvoid_0.0.8_amd64.deb', 60);

    $this->postJson('/api/releases', ['version' => '0.0.8', 'platform' => 'linux-x86_64-deb', 'file' => $first], signedHeaders('/api/releases', $first))->assertCreated();
    $this->postJson('/api/releases', ['version' => '0.0.8', 'platform' => 'linux-x86_64-deb', 'file' => $second], signedHeaders('/api/releases', $second))->assertSuccessful();

    expect(Release::query()->count())->toBe(1)->and(Release::query()->firstOrFail()->size)->toBe(60 * 1024);
});

it('o latest.json ignora o .dmg e o que não tem assinatura', function (): void {
    Release::query()->create(['version' => '0.0.8', 'platform' => ReleasePlatformEnum::MacosDmg, 'file_name' => 'a.dmg', 'path' => 'releases/0.0.8/a.dmg', 'size' => 1, 'signature' => 'x', 'published_at' => now()]);
    Release::query()->create(['version' => '0.0.8', 'platform' => ReleasePlatformEnum::LinuxDeb, 'file_name' => 'a.deb', 'path' => 'releases/0.0.8/a.deb', 'size' => 1, 'signature' => null, 'published_at' => now()]);

    $this->getJson('/downloads/latest.json')->assertNotFound();
});

it('o latest.json deixa de fora a plataforma que ficou numa versão antiga', function (): void {
    Release::query()->create(['version' => '0.0.7', 'platform' => ReleasePlatformEnum::WindowsMsi, 'file_name' => 'a.msi', 'path' => 'releases/0.0.7/a.msi', 'size' => 1, 'signature' => 'assinatura-windows', 'published_at' => now()->subDay()]);
    Release::query()->create(['version' => '0.0.14', 'platform' => ReleasePlatformEnum::MacosApp, 'file_name' => 'a.tar.gz', 'path' => 'releases/0.0.14/a.tar.gz', 'size' => 1, 'signature' => 'assinatura-macos', 'published_at' => now()]);

    $this->getJson('/downloads/latest.json')
        ->assertOk()
        ->assertJsonPath('version', '0.0.14')
        ->assertJsonPath('platforms.darwin-aarch64.signature', 'assinatura-macos')
        ->assertJsonMissingPath('platforms.windows-x86_64-msi')
        ->assertJsonMissingPath('platforms.windows-x86_64');
});

it('a landing mostra a versão e os links das plataformas publicadas', function (): void {
    Release::query()->create(['version' => '0.0.8', 'platform' => ReleasePlatformEnum::LinuxDeb, 'file_name' => 'a.deb', 'path' => 'releases/0.0.8/a.deb', 'size' => 1, 'signature' => null, 'published_at' => now()]);

    $this->get('/')->assertOk()->assertSee('v0.0.8')->assertSee(route('downloads.platform', 'linux'))->assertSee('Baixar o .deb');
});

it('o painel publica pelo formulário e apaga do bucket', function (): void {
    $this->seed(Seeder001Roles::class);
    $admin = User::factory()->create(['email' => config('unkvoid.admin_email')]);

    $this->actingAs($admin)->get(route('admin'))->assertOk()->assertSee('Versões do app');

    Livewire::actingAs($admin)->test(Index::class)
        ->set('version', '0.0.9')
        ->set('platform', 'windows-x86_64-nsis')
        ->set('file', UploadedFile::fake()->create('Unkvoid_0.0.9_x64-setup.exe', 100))
        ->set('signatureFile', UploadedFile::fake()->createWithContent('Unkvoid.exe.sig', "assinatura\n"))
        ->call('publish')
        ->assertHasNoErrors();

    $release = Release::query()->firstOrFail();
    expect($release->signature)->toBe('assinatura');
    Storage::disk('s3')->assertExists($release->path);

    Livewire::actingAs($admin)->test(Index::class)->call('remove', $release->id);
    Storage::disk('s3')->assertMissing($release->path);
    expect(Release::query()->count())->toBe(0);
});
