<?php

declare(strict_types=1);

use App\Enums\ReleasePlatformEnum;
use App\Models\Release;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\Storage;

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

it('abre a página inicial com o download', function (): void {
    $this->get(route('home'))
        ->assertOk()
        ->assertSee('Baixar Unkvoid')
        ->assertSee('apt install unkvoid');
});

it('abre a política de privacidade', function (): void {
    $this->get(route('privacy'))
        ->assertOk()
        ->assertSee('Política de privacidade');
});

it('abre os termos de uso', function (): void {
    $this->get(route('terms'))
        ->assertOk()
        ->assertSee('Termos de uso');
});

it('abre a política de assinatura de código com a frase, os papéis e a privacidade que a SignPath exige', function (): void {
    $this->get('/code-signing-policy')
        ->assertOk()
        ->assertSee('<h1>Code signing policy</h1>', false)
        ->assertSeeText('Free code signing provided by SignPath.io, certificate by SignPath Foundation')
        ->assertSee('<a href="https://about.signpath.io">SignPath.io</a>', false)
        ->assertSee('<a href="https://signpath.org">SignPath Foundation</a>', false)
        ->assertSeeInOrder(['Committers and reviewers', 'https://github.com/edsuuu', 'Approvers', 'https://github.com/edsuuu'])
        ->assertSee('https://github.com/edsuuu/unkvoid')
        ->assertSee('<a href="'.route('privacy').'" wire:navigate>Política de privacidade</a>', false);
});

it('a página inicial aponta para a política de assinatura de código nos downloads e no rodapé', function (): void {
    $this->get(route('home'))
        ->assertOk()
        ->assertSee('<a href="'.route('code-signing').'" class="lp-link" wire:navigate>Code signing policy</a>', false)
        ->assertSee('<a href="'.route('code-signing').'">Code signing policy</a>', false);
});
