<?php

declare(strict_types=1);

use App\Models\File;
use App\Models\User;
use Illuminate\Http\UploadedFile;

it('a foto sobe para `files`, troca apagando a antiga, some e devolve a vez para a do Google', function (): void {
    $commands = fakeS3Client();
    $google = 'https://lh3.googleusercontent.com/foto';
    $user = User::factory()->create(['avatar_url' => $google]);

    $this->actingAs($user, 'sanctum')->getJson('/api/me')
        ->assertOk()
        ->assertJsonPath('data.avatar_url', $google)
        ->assertJsonPath('data.avatar_uploaded', false);

    $this->actingAs($user, 'sanctum')
        ->postJson('/api/me/avatar', ['avatar' => UploadedFile::fake()->image('eu.jpg')])
        ->assertOk()
        ->assertJsonPath('data.avatar_uploaded', true)
        ->assertJsonPath('data.avatar_url', fn (?string $url): bool => ! is_null($url) && ! str_contains($url, 'googleusercontent'));

    $first = File::query()->sole();

    expect($user->refresh()->avatar_id)->toBe($first->id)
        ->and($first->path)->toStartWith("avatars/{$user->id}/")
        ->and($first->mime_type)->toBe('image/jpeg')
        ->and($first->size)->toBeGreaterThan(0)
        ->and($commands->getArrayCopy())->toContain('PutObject')
        ->and($commands->getArrayCopy())->not->toContain('DeleteObject');

    $this->actingAs($user, 'sanctum')
        ->postJson('/api/me/avatar', ['avatar' => UploadedFile::fake()->image('outra.png')])
        ->assertOk();

    expect($user->refresh()->avatar_id)->not->toBe($first->id)
        ->and(File::query()->count())->toBe(1)
        ->and($commands->getArrayCopy())->toContain('DeleteObject');

    $this->actingAs($user, 'sanctum')->deleteJson('/api/me/avatar')
        ->assertOk()
        ->assertJsonPath('data.avatar_url', $google)
        ->assertJsonPath('data.avatar_uploaded', false);

    expect($user->refresh()->avatar_id)->toBeNull()
        ->and(File::query()->count())->toBe(0);
});

it('a foto é sempre a de quem está logado, e só imagem de até 2 MB entra', function (): void {
    fakeS3Client();
    $user = User::factory()->create();

    $this->postJson('/api/me/avatar', ['avatar' => UploadedFile::fake()->image('eu.jpg')])->assertUnauthorized();
    $this->actingAs($user, 'sanctum')->postJson('/api/me/avatar')->assertUnprocessable();
    $this->actingAs($user, 'sanctum')
        ->postJson('/api/me/avatar', ['avatar' => UploadedFile::fake()->create('livro.pdf', 10, 'application/pdf')])
        ->assertUnprocessable();
    $this->actingAs($user, 'sanctum')
        ->postJson('/api/me/avatar', ['avatar' => UploadedFile::fake()->create('enorme.jpg', 2100, 'image/jpeg')])
        ->assertUnprocessable();

    expect(File::query()->count())->toBe(0);
});

it('a primeira foto cria o bucket que falta, e o comando cria o bucket à mão', function (): void {
    $user = User::factory()->create();
    $commands = fakeS3Client(bucketExists: false);

    $this->actingAs($user, 'sanctum')->postJson('/api/me/avatar', ['avatar' => UploadedFile::fake()->image('eu.jpg')])->assertOk();

    expect(array_slice($commands->getArrayCopy(), 0, 2))->toBe(['HeadBucket', 'CreateBucket']);

    $this->artisan('storage:bucket')->expectsOutput('Bucket criado.')->assertSuccessful();

    $commands = fakeS3Client(bucketExists: true);

    $this->artisan('storage:bucket')->expectsOutput('O bucket já existia.')->assertSuccessful();

    expect($commands->getArrayCopy())->toBe(['HeadBucket']);
});
