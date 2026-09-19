<?php

declare(strict_types=1);

use App\Livewire\Auth\ForgotPassword;
use App\Livewire\Auth\Login;
use App\Livewire\Auth\Register;
use App\Livewire\Auth\ResetPassword;
use App\Models\File;
use App\Models\User;
use App\Notifications\NewLoginNotification;
use App\Notifications\ResetPasswordNotification;
use App\Notifications\WelcomeNotification;
use Database\Seeders\Seeder001Roles;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\Notification;
use Illuminate\Support\Facades\Password;
use Laravel\Socialite\Facades\Socialite;
use Laravel\Socialite\Two\User as GoogleUser;
use Livewire\Livewire;

it('entra pela API e recebe um token', function (): void {
    User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    $response = $this->postJson('/api/auth/login', [
        'email' => 'edson@unkvoid.test',
        'password' => 'senha-forte-123',
        'device' => 'desktop-linux',
    ]);

    $response->assertOk()->assertJsonStructure(['data' => ['token', 'user' => ['id', 'name', 'email', 'admin']]]);

    $token = $response->json('data.token');

    $this->withToken($token)->getJson('/api/me')->assertOk()->assertJsonPath('data.email', 'edson@unkvoid.test');
});

it('recusa credenciais erradas na API com 401', function (): void {
    User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    $this->postJson('/api/auth/login', ['email' => 'edson@unkvoid.test', 'password' => 'x', 'device' => 'd'])
        ->assertStatus(401)
        ->assertJsonPath('message', 'E-mail ou senha não conferem.');
});

it('conta só do Google não entra por senha', function (): void {
    User::factory()->create(['email' => 'g@unkvoid.test', 'password' => null, 'google_id' => '123']);

    $this->postJson('/api/auth/login', ['email' => 'g@unkvoid.test', 'password' => 'qualquer', 'device' => 'd'])
        ->assertStatus(401);
});

it('cria a conta pela API e revoga o token no logout', function (): void {
    $response = $this->postJson('/api/auth/register', [
        'email' => 'novo@unkvoid.test',
        'password' => 'senha-forte-123',
        'device' => 'desktop-windows',
    ]);

    $response->assertCreated()->assertJsonPath('data.user.name', 'novo');

    $token = $response->json('data.token');

    $this->withToken($token)->postJson('/api/auth/logout')->assertNoContent();
    $this->assertDatabaseCount('personal_access_tokens', 0);
});

it('exige token para o /api/me', function (): void {
    $this->getJson('/api/me')->assertUnauthorized();
});

it('o cadastro pela API tira o apelido do e-mail, sem confirmar, e desempata quando já existe', function (): void {
    User::factory()->create(['name' => 'edson.lima']);

    $this->postJson('/api/auth/register', ['email' => 'Edson.Lima@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd'])
        ->assertCreated()
        ->assertJsonPath('data.user.name', 'edson.lima2')
        ->assertJsonPath('data.user.nickname_confirmed', false);

    expect(User::query()->where('email', 'edson.lima@unkvoid.test')->firstOrFail()->nickname_confirmed_at)->toBeNull();
});

it('o /api/me diz se o apelido já foi confirmado', function (): void {
    $this->actingAs(User::factory()->create())->getJson('/api/me')->assertOk()->assertJsonPath('data.nickname_confirmed', true);
    $this->actingAs(User::factory()->unconfirmedNickname()->create())->getJson('/api/me')->assertOk()->assertJsonPath('data.nickname_confirmed', false);
});

it('escolhe o apelido pelo PATCH /api/me e confirma', function (): void {
    $user = User::factory()->unconfirmedNickname()->create(['name' => 'novo']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'edsu.dev'])
        ->assertOk()
        ->assertJsonPath('data.name', 'edsu.dev')
        ->assertJsonPath('data.nickname_confirmed', true);

    expect($user->fresh()?->hasConfirmedNickname())->toBeTrue();
});

it('ficar com o apelido automático também confirma', function (): void {
    $user = User::factory()->unconfirmedNickname()->create(['name' => 'novo']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'novo'])
        ->assertOk()
        ->assertJsonPath('data.name', 'novo')
        ->assertJsonPath('data.nickname_confirmed', true);
});

it('não troca o apelido de quem já confirmou', function (): void {
    $user = User::factory()->create(['name' => 'edsu']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'outro.nome'])
        ->assertForbidden()
        ->assertJsonPath('message', 'Você já escolheu o seu apelido.');

    expect($user->fresh()?->name)->toBe('edsu');
});

it('o PATCH /api/me exige token', function (): void {
    $this->patchJson('/api/me', ['name' => 'edsu'])->assertUnauthorized();
});

it('abre as telas de entrar e de criar conta', function (): void {
    $this->get(route('login'))->assertOk()->assertSee('Entrar com Google');
    $this->get(route('register'))->assertOk()->assertSee('Criar conta com Google');
});

it('entra com e-mail e senha pelo site', function (): void {
    $user = User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    Livewire::test(Login::class)
        ->set('email', 'Edson@unkvoid.test')
        ->set('password', 'senha-forte-123')
        ->call('login')
        ->assertHasNoErrors()
        ->assertRedirect(route('home', absolute: false));

    $this->assertAuthenticatedAs($user);
});

it('recusa senha errada sem dizer qual campo errou', function (): void {
    User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    Livewire::test(Login::class)
        ->set('email', 'edson@unkvoid.test')
        ->set('password', 'outra')
        ->call('login')
        ->assertHasErrors(['email']);

    $this->assertGuest();
});

it('cria a conta pelo site e já entra', function (): void {
    Livewire::test(Register::class)
        ->set('name', 'Edson')
        ->set('email', 'novo@unkvoid.test')
        ->set('password', 'senha-forte-123')
        ->set('password_confirmation', 'senha-forte-123')
        ->call('register')
        ->assertHasNoErrors()
        ->assertRedirect(route('home', absolute: false));

    $this->assertAuthenticated();
    $this->assertDatabaseHas('users', ['email' => 'novo@unkvoid.test']);
    expect(User::query()->where('email', 'novo@unkvoid.test')->firstOrFail()->hasConfirmedNickname())->toBeTrue();
});

it('sai da conta', function (): void {
    $user = User::factory()->create();

    $this->actingAs($user)->post(route('logout'))->assertRedirect(route('home'));
    $this->assertGuest();
});

it('só o e-mail do dono nasce administrador', function (): void {
    $this->seed(Seeder001Roles::class);

    $common = User::factory()->create();
    $this->actingAs($common, 'sanctum')->getJson('/api/me')->assertOk()->assertJsonPath('data.admin', false);

    $admin = User::factory()->create(['email' => config('unkvoid.admin_email')]);
    $this->actingAs($admin, 'sanctum')->getJson('/api/me')->assertOk()->assertJsonPath('data.admin', true);
});

function googleUser(string $id, string $email, string $name): GoogleUser
{
    $user = new GoogleUser;
    $user->map(['id' => $id, 'email' => $email, 'name' => $name, 'avatar' => 'https://lh3.example/foto.png']);

    return $user;
}

it('redireciona para o Google', function (): void {
    $this->get(route('oauth2.google'))->assertRedirect();
});

it('cria a conta na primeira entrada com o Google', function (): void {
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-1', 'Edson@Unkvoid.test', 'Edson'));

    $this->get(route('oauth2.google.callback'))->assertRedirect(route('home', absolute: false));

    $this->assertAuthenticated();
    $this->assertDatabaseHas('users', ['email' => 'edson@unkvoid.test', 'google_id' => 'g-1']);
    expect(User::query()->firstOrFail()->hasConfirmedNickname())->toBeFalse();
});

it('vincula o Google a uma conta que já existia pelo e-mail', function (): void {
    $existing = User::factory()->create(['email' => 'edson@unkvoid.test']);
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-2', 'edson@unkvoid.test', 'Edson'));

    $this->get(route('oauth2.google.callback'));

    $this->assertAuthenticatedAs($existing);
    expect($existing->fresh()->google_id)->toBe('g-2');
    expect($existing->fresh()->hasConfirmedNickname())->toBeTrue();
    $this->assertDatabaseCount('users', 1);
});

it('devolve o token para o app pela porta local', function (): void {
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-3', 'app@unkvoid.test', 'App'));

    $this->get(route('oauth2.app', ['port' => 43123]))->assertSessionHasErrors('state');
    $this->get(route('oauth2.app', ['port' => 43123, 'state' => 'zz']))->assertSessionHasErrors('state');
    $this->get(route('oauth2.app', ['port' => 43123, 'state' => 'c0ffee42']))->assertRedirect(route('oauth2.google'));

    $response = $this->get(route('oauth2.google.callback'));

    $response->assertRedirect();

    expect($response->headers->get('Location'))->toStartWith('http://127.0.0.1:43123/?token=')->toEndWith('&state=c0ffee42');
    $this->assertDatabaseCount('personal_access_tokens', 1);
});

it('devolve o token para o app pelo unkvoid:// quando ele não manda porta', function (): void {
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-5', 'deeplink@unkvoid.test', 'App'));

    $this->get(route('oauth2.app', ['state' => 'c0ffee42']))->assertRedirect(route('oauth2.google'));

    $this->get(route('oauth2.google.callback'))
        ->assertOk()
        ->assertViewIs('auth.app-return')
        ->assertSee('unkvoid://login?token=', false);

    $this->assertDatabaseCount('personal_access_tokens', 1);
});

it('quem entra com o e-mail do dono vira administrador', function (): void {
    $this->seed(Seeder001Roles::class);
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-4', config('unkvoid.admin_email'), 'Dono'));

    $this->get(route('oauth2.google.callback'));

    expect(User::query()->firstOrFail()->isAdmin())->toBeTrue();
});

it('manda o link de redefinição por e-mail e não revela se a conta existe', function (): void {
    Notification::fake();
    $user = User::factory()->create(['email' => 'edson@unkvoid.test']);

    Livewire::test(ForgotPassword::class)->set('email', 'edson@unkvoid.test')->call('send')->assertSet('sent', true);
    Livewire::test(ForgotPassword::class)->set('email', 'ninguem@unkvoid.test')->call('send')->assertSet('sent', true);

    Notification::assertSentTo($user, ResetPasswordNotification::class);
    Notification::assertCount(1);
});

it('redefine a senha com o token e já entra', function (): void {
    $user = User::factory()->create(['email' => 'edson@unkvoid.test']);
    $token = Password::broker()->createToken($user);

    $this->get(route('password.reset', ['token' => $token, 'email' => 'edson@unkvoid.test']))->assertOk()->assertSee('Nova senha');

    Livewire::test(ResetPassword::class, ['token' => $token, 'email' => 'edson@unkvoid.test'])
        ->set('password', 'senha-nova-123')
        ->set('password_confirmation', 'senha-nova-123')
        ->call('save')
        ->assertHasNoErrors()
        ->assertRedirect(route('home', absolute: false));

    expect(Hash::check('senha-nova-123', $user->fresh()->password))->toBeTrue();
    $this->assertAuthenticatedAs($user);
});

it('recusa um token inválido', function (): void {
    User::factory()->create(['email' => 'edson@unkvoid.test']);

    Livewire::test(ResetPassword::class, ['token' => 'lixo', 'email' => 'edson@unkvoid.test'])
        ->set('password', 'senha-nova-123')
        ->set('password_confirmation', 'senha-nova-123')
        ->call('save')
        ->assertHasErrors(['email']);
});

it('manda boas-vindas ao criar a conta pelo site e pela API', function (): void {
    Notification::fake();

    Livewire::test(Register::class)
        ->set('name', 'edson.site')->set('email', 'site@unkvoid.test')
        ->set('password', 'senha-forte-123')->set('password_confirmation', 'senha-forte-123')
        ->call('register');

    $this->postJson('/api/auth/register', ['email' => 'api@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd'])->assertCreated();

    Notification::assertSentTo(User::query()->where('email', 'site@unkvoid.test')->firstOrFail(), WelcomeNotification::class);
    Notification::assertSentTo(User::query()->where('email', 'api@unkvoid.test')->firstOrFail(), WelcomeNotification::class);
});

it('avisa de um novo acesso pelo site, pela API e pelo Google', function (): void {
    Notification::fake();
    $user = User::factory()->create(['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123']);

    Livewire::test(Login::class)->set('email', 'edson@unkvoid.test')->set('password', 'senha-forte-123')->call('login');
    $this->postJson('/api/auth/login', ['email' => 'edson@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'notebook'])->assertOk();

    $googleUser = new GoogleUser;
    $googleUser->map(['id' => 'g-9', 'email' => 'edson@unkvoid.test', 'name' => 'Edson', 'avatar' => null]);
    Socialite::shouldReceive('driver->user')->andReturn($googleUser);
    $this->get(route('oauth2.google.callback'));

    Notification::assertSentToTimes($user, NewLoginNotification::class, 3);
});

it('renderiza os três e-mails em HTML com o desenho do site', function (): void {
    $user = User::factory()->create(['name' => 'Edson', 'email' => 'edson@unkvoid.test']);

    $welcome = (new WelcomeNotification)->toMail($user)->render();
    expect((string) $welcome)->toContain('Bem-vindo, Edson.')->toContain('<table')->toContain('unkvoid-mark.png')->toContain('#06050a');

    $login = new NewLoginNotification('site', '203.0.113.9', 'Firefox')->toMail($user)->render();
    expect((string) $login)->toContain('Novo acesso')->toContain('203.0.113.9')->toContain(route('password.request'));

    $reset = new ResetPasswordNotification('abc')->toMail($user)->render();
    expect((string) $reset)->toContain('Redefinir')->toContain(route('password.reset', ['token' => 'abc', 'email' => 'edson@unkvoid.test']));
});

it('o apelido e unico e nao aceita espaco', function (): void {
    User::factory()->create(['name' => 'edsu']);
    $user = User::factory()->unconfirmedNickname()->create(['name' => 'outro']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'edsu'])
        ->assertStatus(422)
        ->assertJsonPath('errors.name.0', 'Esse apelido já é de outra pessoa.');

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'edsu lima'])
        ->assertStatus(422)
        ->assertJsonPath('errors.name.0', 'O apelido aceita letras, números, ponto e _ — sem espaço.');

    expect($user->fresh()?->hasConfirmedNickname())->toBeFalse();

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'edsu.dois'])->assertOk();
});

it('o login pelo google vira apelido sem espaco, e desempata quando ja existe', function (): void {
    expect(User::freeNickname('Edson Lima'))->toBe('edsonlima');

    User::factory()->create(['name' => 'edsonlima']);

    expect(User::freeNickname('Edson Lima'))->toBe('edsonlima2');
    expect(User::freeNickname('Çá'))->toBe('pessoa');
});

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
