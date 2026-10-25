<?php

declare(strict_types=1);

use App\Enums\FriendshipStatusEnum;
use App\Livewire\Auth\ForgotPassword;
use App\Livewire\Auth\Login;
use App\Livewire\Auth\Register;
use App\Livewire\Auth\ResetPassword;
use App\Models\File;
use App\Models\Friendship;
use App\Models\Server;
use App\Models\User;
use App\Notifications\NewLoginNotification;
use App\Notifications\ResetPasswordNotification;
use App\Notifications\WelcomeNotification;
use Database\Seeders\Seeder001Roles;
use Illuminate\Http\UploadedFile;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Notification;
use Illuminate\Support\Facades\Password;
use Illuminate\Support\Str;
use Illuminate\Testing\TestResponse;
use Laravel\Socialite\Facades\Socialite;
use Laravel\Socialite\Two\User as GoogleUser;
use Livewire\Livewire;

it('entra pela API e recebe um token', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123']);

    $response = $this->postJson('/api/auth/login', [
        'email' => 'ada@unkvoid.test',
        'password' => 'senha-forte-123',
        'device' => 'desktop-linux',
    ]);

    $response->assertOk()->assertJsonStructure(['data' => ['token', 'user' => ['id', 'name', 'email', 'admin']]]);

    $token = $response->json('data.token');

    $this->withToken($token)->getJson('/api/me')->assertOk()->assertJsonPath('data.email', 'ada@unkvoid.test');
});

it('recusa credenciais erradas na API com 401', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123']);

    $this->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => 'x', 'device' => 'd'])
        ->assertStatus(401)
        ->assertJsonPath('message', 'E-mail, apelido ou senha não conferem.');
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

it('entra com o nome e a senha: o apelido sai do nome, o e-mail é de mentira e nada é enviado', function (): void {
    Notification::fake();
    User::factory()->create(['name' => 'joaosilva']);

    $this->postJson('/api/auth/start', ['name' => 'João Silva', 'device' => 'd'])
        ->assertJsonValidationErrors(['password']);

    $response = $this->postJson('/api/auth/start', ['name' => 'João Silva', 'password' => 'senha-forte-123', 'device' => 'desktop-windows', 'refresh' => true])
        ->assertCreated()
        ->assertJsonPath('data.user.name', 'joaosilva2')
        ->assertJsonPath('data.user.nickname_confirmed', true)
        ->assertJsonPath('data.user.name_only', true);

    $user = User::query()->where('name', 'joaosilva2')->sole();

    expect($user->email)->toMatch('/^joaosilva2@[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}\.unkvoid\.com$/')
        ->and($response->json('data.refresh_token'))->toBeString()
        // O canal de e-mail pula quem não tem endereço: boas-vindas, aviso de login e troca
        // de senha não saem para o de mentira. O fake não olha o endereço; o canal olha.
        ->and($user->routeNotificationFor('mail'))->toBeNull()
        ->and(User::query()->where('name', 'joaosilva')->sole()->routeNotificationFor('mail'))->toBeString();

    Notification::assertNothingSent();
});

it('a conta com o nome entra pelo apelido e a senha, e põe o e-mail uma vez', function (): void {
    $token = $this->postJson('/api/auth/start', ['name' => 'Ada', 'password' => 'senha-forte-123', 'device' => 'd'])->json('data.token');

    $this->postJson('/api/auth/login', ['email' => ' Ada ', 'password' => 'senha-forte-123', 'device' => 'd'])->assertOk();
    $this->postJson('/api/auth/login', ['email' => 'ada', 'password' => 'errada-123', 'device' => 'd'])->assertStatus(401);

    $this->withToken($token)->postJson('/api/me/email', ['email' => 'Ada@Unkvoid.test'])
        ->assertOk()
        ->assertJsonPath('data.email', 'ada@unkvoid.test')
        ->assertJsonPath('data.name_only', false);

    $this->withToken($token)->postJson('/api/me/email', ['email' => 'outro@unkvoid.test'])
        ->assertUnprocessable()
        ->assertJsonValidationErrors(['email' => 'Esta conta já tem e-mail.']);

    $this->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd'])->assertOk();
});

it('o e-mail que a conta com o nome escolhe não pode ser de outra', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test']);
    $token = $this->postJson('/api/auth/start', ['name' => 'Ada', 'password' => 'senha-forte-123', 'device' => 'd'])->json('data.token');

    $this->withToken($token)->postJson('/api/me/email', ['email' => 'ada@unkvoid.test'])
        ->assertJsonValidationErrors(['email' => 'Esse e-mail já é de outra conta.']);
});

it('o app que sabe renovar recebe o par, e o de renovação só serve para trocar o par', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123']);

    $login = $this->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd', 'refresh' => true])
        ->assertOk()
        ->assertJsonStructure(['data' => ['token', 'refresh_token', 'expires_at']]);

    [$access, $refresh] = [$login->json('data.token'), $login->json('data.refresh_token')];

    $this->withToken($access)->getJson('/api/me')->assertOk();
    $this->app['auth']->forgetGuards();
    $this->withToken($refresh)->getJson('/api/me')->assertUnauthorized();
    $this->app['auth']->forgetGuards();

    $renewed = $this->postJson('/api/auth/refresh', ['refresh_token' => $refresh])->assertOk();

    expect($renewed->json('data.refresh_token'))->not->toBe($refresh);

    $this->postJson('/api/auth/refresh', ['refresh_token' => $refresh])
        ->assertStatus(401)
        ->assertJsonPath('message', 'Sua sessão terminou. Entre de novo.');
    $this->withToken($renewed->json('data.token'))->getJson('/api/me')->assertOk();
});

it('o token de acesso vence em um dia e o de renovação o traz de volta', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123']);

    $login = $this->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd', 'refresh' => true]);

    $this->travel(25)->hours();

    $this->withToken($login->json('data.token'))->getJson('/api/me')->assertUnauthorized();
    $this->app['auth']->forgetGuards();

    $renewed = $this->postJson('/api/auth/refresh', ['refresh_token' => $login->json('data.refresh_token')])->assertOk();

    $this->withToken($renewed->json('data.token'))->getJson('/api/me')->assertOk();

    $this->travel(61)->days();

    $this->postJson('/api/auth/refresh', ['refresh_token' => $renewed->json('data.refresh_token')])->assertStatus(401);
});

it('o app antigo, que não pede renovação, continua com o token que não vence', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123']);

    $login = $this->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd'])
        ->assertOk()
        ->assertJsonPath('data.refresh_token', null);

    $this->travel(400)->days();

    $this->withToken($login->json('data.token'))->getJson('/api/me')->assertOk();
});

it('sair da conta derruba também o token de renovação', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123']);

    $login = $this->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd', 'refresh' => true]);

    $this->withToken($login->json('data.token'))
        ->postJson('/api/auth/logout', ['refresh_token' => $login->json('data.refresh_token')])
        ->assertNoContent();

    $this->assertDatabaseCount('personal_access_tokens', 0);
});

it('exige token para o /api/me', function (): void {
    $this->getJson('/api/me')->assertUnauthorized();
});

it('o cadastro pela API tira o apelido do e-mail, sem confirmar, e desempata quando já existe', function (): void {
    User::factory()->create(['name' => 'ada.byron']);

    $this->postJson('/api/auth/register', ['email' => 'Ada.Byron@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd'])
        ->assertCreated()
        ->assertJsonPath('data.user.name', 'ada.byron2')
        ->assertJsonPath('data.user.nickname_confirmed', false);

    expect(User::query()->where('email', 'ada.byron@unkvoid.test')->firstOrFail()->nickname_confirmed_at)->toBeNull();
});

it('o /api/me diz se o apelido já foi confirmado', function (): void {
    $this->actingAs(User::factory()->create())->getJson('/api/me')->assertOk()->assertJsonPath('data.nickname_confirmed', true);
    $this->actingAs(User::factory()->unconfirmedNickname()->create())->getJson('/api/me')->assertOk()->assertJsonPath('data.nickname_confirmed', false);
});

it('escolhe o apelido pelo PATCH /api/me e confirma', function (): void {
    $user = User::factory()->unconfirmedNickname()->create(['name' => 'novo']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'ada.dev'])
        ->assertOk()
        ->assertJsonPath('data.name', 'ada.dev')
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

it('quem já confirmou troca o apelido de novo, e a data da escolha fica', function (): void {
    $user = User::factory()->create(['name' => 'ada', 'nickname_confirmed_at' => now()->subYear()]);
    $chosen = $user->nickname_confirmed_at?->toDateTimeString();

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'outro.nome'])
        ->assertOk()
        ->assertJsonPath('data.name', 'outro.nome')
        ->assertJsonPath('data.nickname_confirmed', true);

    expect($user->fresh()?->name)->toBe('outro.nome')
        ->and($user->fresh()?->nickname_confirmed_at?->toDateTimeString())->toBe($chosen);
});

it('o PATCH /api/me troca o perfil sem mexer no apelido, e null limpa', function (): void {
    $user = User::factory()->create(['name' => 'ada']);

    $this->actingAs($user)->patchJson('/api/me', ['bio' => 'Escrevo código', 'pronouns' => 'ela/dela', 'banner_color' => '#5865f2', 'custom_status' => 'jogando'])
        ->assertOk()
        ->assertJsonPath('data.name', 'ada')
        ->assertJsonPath('data.bio', 'Escrevo código')
        ->assertJsonPath('data.pronouns', 'ela/dela')
        ->assertJsonPath('data.banner_color', '#5865f2')
        ->assertJsonPath('data.custom_status', 'jogando');

    $this->actingAs($user)->patchJson('/api/me', ['bio' => null])
        ->assertOk()
        ->assertJsonPath('data.bio', null)
        ->assertJsonPath('data.pronouns', 'ela/dela');

    $this->actingAs($user)->patchJson('/api/me', ['banner_color' => 'azul'])->assertUnprocessable()->assertJsonValidationErrors(['banner_color']);
    $this->actingAs($user)->patchJson('/api/me', ['bio' => str_repeat('a', 191)])->assertUnprocessable()->assertJsonValidationErrors(['bio']);
});

it('o PATCH /api/me exige token', function (): void {
    $this->patchJson('/api/me', ['name' => 'ada'])->assertUnauthorized();
});

it('salva status e prazo pelo PATCH /api/me e o status vencido volta a disponível', function (): void {
    $user = User::factory()->create();

    $this->actingAs($user, 'sanctum')->patchJson('/api/me', ['status' => 'idle', 'status_minutes' => 15])
        ->assertOk()
        ->assertJsonPath('data.status', 'idle');

    expect($user->fresh()->status_until)->not->toBeNull();

    $this->travel(16)->minutes();

    $this->actingAs($user, 'sanctum')->getJson('/api/me')->assertOk()->assertJsonPath('data.status', 'online');

    $this->actingAs($user, 'sanctum')->patchJson('/api/me', ['status' => 'dnd', 'status_minutes' => null])
        ->assertOk()
        ->assertJsonPath('data.status', 'dnd');

    expect($user->fresh()->status_until)->toBeNull();
});

it('mostra como offline para outra pessoa quem escolheu invisível', function (): void {
    fakeSfu();
    $invisible = User::factory()->create(['status' => 'invisible']);
    $viewer = User::factory()->create();
    $server = Server::createFor($viewer, 'Casa');
    joinServer($server, $invisible);
    $friendship = Friendship::query()->create([
        'requester_id' => $invisible->id,
        'addressee_id' => $viewer->id,
        'status' => FriendshipStatusEnum::Accepted,
        'responded_at' => now(),
    ]);

    $this->actingAs($viewer, 'sanctum')->getJson('/api/friends')
        ->assertOk()
        ->assertJsonPath('data.0.requester.status', 'offline')
        ->assertJsonPath('data.0.addressee.status', 'online');

    $this->actingAs($viewer, 'sanctum')->getJson("/api/servers/{$server->id}")
        ->assertOk()
        ->assertJsonFragment(['user_id' => $invisible->id, 'status' => 'offline']);

    expect($friendship->requester->effectiveStatus())->toBe('invisible');
});

it('compartilha atividade efêmera só com amizades aceitas e limpa ao receber nulo', function (): void {
    Http::fake();
    $owner = User::factory()->create();
    $friend = User::factory()->create();
    $pending = User::factory()->create();
    Friendship::query()->create([
        'requester_id' => $owner->id,
        'addressee_id' => $friend->id,
        'status' => FriendshipStatusEnum::Accepted,
        'responded_at' => now(),
    ]);
    Friendship::query()->create([
        'requester_id' => $owner->id,
        'addressee_id' => $pending->id,
        'status' => FriendshipStatusEnum::Pending,
    ]);

    $this->actingAs($owner, 'sanctum')->putJson('/api/me/activity', [
        'activity' => ['name' => 'Arena Breakout: Infinite', 'started_at' => now()->toIso8601String()],
    ])->assertNoContent();

    $this->actingAs($friend, 'sanctum')->getJson('/api/friends')
        ->assertOk()
        ->assertJsonPath('data.0.requester.activity.name', 'Arena Breakout: Infinite');

    $this->actingAs($pending, 'sanctum')->getJson('/api/friends')
        ->assertOk()
        ->assertJsonPath('data.0.requester.activity', null);

    $this->actingAs($owner, 'sanctum')->putJson('/api/me/activity', ['activity' => null])->assertNoContent();

    $this->actingAs($friend, 'sanctum')->getJson('/api/friends')
        ->assertOk()
        ->assertJsonPath('data.0.requester.activity', null);
});

it('não expõe atividade para quem escolheu invisível', function (): void {
    Http::fake();
    $invisible = User::factory()->create(['status' => 'invisible']);
    $friend = User::factory()->create();
    Friendship::query()->create([
        'requester_id' => $invisible->id,
        'addressee_id' => $friend->id,
        'status' => FriendshipStatusEnum::Accepted,
        'responded_at' => now(),
    ]);

    $this->actingAs($invisible, 'sanctum')->putJson('/api/me/activity', [
        'activity' => ['name' => 'Jogo secreto', 'started_at' => now()->toIso8601String()],
    ])->assertNoContent();

    $this->actingAs($friend, 'sanctum')->getJson('/api/friends')
        ->assertOk()
        ->assertJsonPath('data.0.requester.status', 'offline')
        ->assertJsonPath('data.0.requester.activity', null);
});

it('abre as telas de entrar e de criar conta', function (): void {
    $this->get(route('login'))->assertOk()->assertSee('Entrar com Google');
    $this->get(route('register'))->assertOk()->assertSee('Criar conta com Google');
});

it('entra com e-mail e senha pelo site', function (): void {
    $user = User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123']);

    Livewire::test(Login::class)
        ->set('email', 'Ada@unkvoid.test')
        ->set('password', 'senha-forte-123')
        ->call('login')
        ->assertHasNoErrors()
        ->assertRedirect(route('home', absolute: false));

    $this->assertAuthenticatedAs($user);
});

it('recusa senha errada sem dizer qual campo errou', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => 'senha-forte-123']);

    Livewire::test(Login::class)
        ->set('email', 'ada@unkvoid.test')
        ->set('password', 'outra')
        ->call('login')
        ->assertHasErrors(['email']);

    $this->assertGuest();
});

it('cria a conta pelo site e já entra', function (): void {
    Livewire::test(Register::class)
        ->set('name', 'Ada')
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
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-1', 'Ada@Unkvoid.test', 'Ada'));

    $this->get(route('oauth2.google.callback'))->assertRedirect(route('home', absolute: false));

    $this->assertAuthenticated();
    $this->assertDatabaseHas('users', ['email' => 'ada@unkvoid.test', 'google_id' => 'g-1']);
    expect(User::query()->firstOrFail()->hasConfirmedNickname())->toBeFalse();
});

it('vincula o Google a uma conta que já existia pelo e-mail', function (): void {
    $existing = User::factory()->create(['email' => 'ada@unkvoid.test']);
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-2', 'ada@unkvoid.test', 'Ada'));

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

it('o login do Google entrega o par ao app que sabe renovar', function (): void {
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-6', 'par@unkvoid.test', 'App'));

    $this->get(route('oauth2.app', ['port' => 43123, 'state' => 'c0ffee42', 'refresh' => 1]))->assertRedirect(route('oauth2.google'));

    $location = $this->get(route('oauth2.google.callback'))->headers->get('Location');

    expect($location)->toContain('&refresh_token=')->toEndWith('&state=c0ffee42');
    $this->assertDatabaseCount('personal_access_tokens', 2);
});

it('troca o código PKCE do Google uma única vez sem expor tokens na URL', function (): void {
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-pkce', 'pkce@unkvoid.test', 'App'));
    $verifier = 'dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk';
    $challenge = mb_rtrim(strtr(base64_encode(hash('sha256', $verifier, true)), '+/', '-_'), '=');

    $this->get(route('oauth2.app', [
        'port' => 43123,
        'state' => 'c0ffee42',
        'refresh' => 1,
        'code_challenge' => $challenge,
        'code_challenge_method' => 'S256',
    ]))->assertRedirect(route('oauth2.google'));

    $response = $this->get(route('oauth2.google.callback'));
    $location = $response->headers->get('Location');

    expect($location)->toStartWith('http://127.0.0.1:43123/?code=')->toContain('&state=c0ffee42')->not->toContain('token=');
    parse_str((string) parse_url((string) $location, PHP_URL_QUERY), $query);

    $exchange = $this->postJson('/api/auth/app/exchange', ['code' => $query['code'], 'code_verifier' => $verifier])
        ->assertOk()
        ->assertJsonStructure(['data' => ['token', 'refresh_token']]);

    $this->postJson('/api/auth/app/exchange', ['code' => $query['code'], 'code_verifier' => $verifier])
        ->assertUnprocessable()
        ->assertJsonValidationErrors(['code']);

    expect($exchange->json('data.token'))->not->toBeEmpty();
});

it('consome o código PKCE quando o verifier está errado ou o prazo venceu', function (): void {
    $user = User::factory()->create();
    $verifier = 'dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk';
    $challenge = mb_rtrim(strtr(base64_encode(hash('sha256', $verifier, true)), '+/', '-_'), '=');
    $code = 'a'.str_repeat('b', 63);
    Cache::put('app-login-exchange:'.hash('sha256', $code), [
        'user_id' => $user->id,
        'challenge' => $challenge,
        'refresh' => false,
    ], now()->addSeconds(60));

    $this->postJson('/api/auth/app/exchange', ['code' => $code, 'code_verifier' => str_repeat('x', 43)])
        ->assertUnprocessable()
        ->assertJsonValidationErrors(['code']);
    $this->postJson('/api/auth/app/exchange', ['code' => $code, 'code_verifier' => $verifier])
        ->assertUnprocessable()
        ->assertJsonValidationErrors(['code']);

    Cache::put('app-login-exchange:'.hash('sha256', $code), [
        'user_id' => $user->id,
        'challenge' => $challenge,
        'refresh' => false,
    ], now()->addSeconds(60));
    $this->travel(61)->seconds();
    $this->postJson('/api/auth/app/exchange', ['code' => $code, 'code_verifier' => $verifier])
        ->assertUnprocessable()
        ->assertJsonValidationErrors(['code']);
});

it('quem entra com o e-mail do dono vira administrador', function (): void {
    $this->seed(Seeder001Roles::class);
    Socialite::shouldReceive('driver->user')->andReturn(googleUser('g-4', config('unkvoid.admin_email'), 'Dono'));

    $this->get(route('oauth2.google.callback'));

    expect(User::query()->firstOrFail()->isAdmin())->toBeTrue();
});

it('manda o link de redefinição por e-mail e não revela se a conta existe', function (): void {
    Notification::fake();
    $user = User::factory()->create(['email' => 'ada@unkvoid.test']);

    Livewire::test(ForgotPassword::class)->set('email', 'ada@unkvoid.test')->call('send')->assertSet('sent', true);
    Livewire::test(ForgotPassword::class)->set('email', 'ninguem@unkvoid.test')->call('send')->assertSet('sent', true);

    Notification::assertSentTo($user, ResetPasswordNotification::class);
    Notification::assertCount(1);
});

it('redefine a senha com o token e já entra', function (): void {
    $user = User::factory()->create(['email' => 'ada@unkvoid.test']);
    $token = Password::broker()->createToken($user);

    $this->get(route('password.reset', ['token' => $token, 'email' => 'ada@unkvoid.test']))->assertOk()->assertSee('Nova senha');

    Livewire::test(ResetPassword::class, ['token' => $token, 'email' => 'ada@unkvoid.test'])
        ->set('password', 'senha-nova-123')
        ->set('password_confirmation', 'senha-nova-123')
        ->call('save')
        ->assertHasNoErrors()
        ->assertRedirect(route('home', absolute: false));

    expect(Hash::check('senha-nova-123', $user->fresh()->password))->toBeTrue();
    $this->assertAuthenticatedAs($user);
});

it('recusa um token inválido', function (): void {
    User::factory()->create(['email' => 'ada@unkvoid.test']);

    Livewire::test(ResetPassword::class, ['token' => 'lixo', 'email' => 'ada@unkvoid.test'])
        ->set('password', 'senha-nova-123')
        ->set('password_confirmation', 'senha-nova-123')
        ->call('save')
        ->assertHasErrors(['email']);
});

it('manda boas-vindas ao criar a conta pelo site e pela API', function (): void {
    Notification::fake();

    Livewire::test(Register::class)
        ->set('name', 'ada.site')->set('email', 'site@unkvoid.test')
        ->set('password', 'senha-forte-123')->set('password_confirmation', 'senha-forte-123')
        ->call('register');

    $this->postJson('/api/auth/register', ['email' => 'api@unkvoid.test', 'password' => 'senha-forte-123', 'device' => 'd'])->assertCreated();

    Notification::assertSentTo(User::query()->where('email', 'site@unkvoid.test')->firstOrFail(), WelcomeNotification::class);
    Notification::assertSentTo(User::query()->where('email', 'api@unkvoid.test')->firstOrFail(), WelcomeNotification::class);
});

it('avisa de um novo acesso pelo site, pela API e pelo Google', function (): void {
    Notification::fake();
    $password = Str::random(16);
    $user = User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => $password]);

    // Cada porta vem de um navegador diferente, que é o que acontece de verdade: o site num
    // navegador, o app com o `User-Agent` dele, e o Google de volta por uma aba.
    $this->withServerVariables(['REMOTE_ADDR' => '203.0.113.1', 'HTTP_USER_AGENT' => 'Firefox']);
    Livewire::test(Login::class)->set('email', 'ada@unkvoid.test')->set('password', $password)->call('login');

    $this->withHeader('User-Agent', 'Unkvoid/1.0')
        ->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => $password, 'device' => 'notebook'])
        ->assertOk();

    $googleUser = new GoogleUser;
    $googleUser->map(['id' => 'g-9', 'email' => 'ada@unkvoid.test', 'name' => 'Ada', 'avatar' => null]);
    Socialite::shouldReceive('driver->user')->andReturn($googleUser);
    $this->withHeader('User-Agent', 'Chrome')->get(route('oauth2.google.callback'));

    Notification::assertSentToTimes($user, NewLoginNotification::class, 3);
});

it('não avisa de novo quando o acesso vem do mesmo lugar de antes', function (): void {
    Notification::fake();
    $password = Str::random(16);
    $user = User::factory()->create(['email' => 'ada@unkvoid.test', 'password' => $password]);

    $entrar = fn (): TestResponse => $this->withHeader('User-Agent', 'Unkvoid/1.0')
        ->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => $password, 'device' => 'notebook']);

    $entrar()->assertOk();
    $entrar()->assertOk();
    $entrar()->assertOk();

    // Aviso que chega em todo login deixa de ser aviso: a pessoa passa a apagar sem ler.
    Notification::assertSentToTimes($user, NewLoginNotification::class, 1);

    // De outro computador, avisa — que é o caso que importa.
    $this->withHeader('User-Agent', 'Outro navegador')
        ->postJson('/api/auth/login', ['email' => 'ada@unkvoid.test', 'password' => $password, 'device' => 'desktop'])
        ->assertOk();

    Notification::assertSentToTimes($user, NewLoginNotification::class, 2);
});

it('renderiza os três e-mails em HTML com o desenho do site', function (): void {
    $user = User::factory()->create(['name' => 'Ada', 'email' => 'ada@unkvoid.test']);

    $welcome = (new WelcomeNotification)->toMail($user)->render();
    expect((string) $welcome)->toContain('Bem-vindo, Ada.')->toContain('<table')->toContain('unkvoid-mark.png')->toContain('#06050a');

    $login = new NewLoginNotification('site', '203.0.113.9', 'Firefox')->toMail($user)->render();
    expect((string) $login)->toContain('Novo acesso')->toContain('203.0.113.9')->toContain(route('password.request'));

    $reset = new ResetPasswordNotification('abc')->toMail($user)->render();
    expect((string) $reset)->toContain('Redefinir')->toContain(route('password.reset', ['token' => 'abc', 'email' => 'ada@unkvoid.test']));
});

it('o apelido e unico e nao aceita espaco', function (): void {
    User::factory()->create(['name' => 'ada']);
    $user = User::factory()->unconfirmedNickname()->create(['name' => 'outro']);

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'ada'])
        ->assertStatus(422)
        ->assertJsonPath('errors.name.0', 'Esse apelido já é de outra pessoa.');

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'ada byron'])
        ->assertStatus(422)
        ->assertJsonPath('errors.name.0', 'O apelido aceita letras, números, ponto e _ — sem espaço.');

    expect($user->fresh()?->hasConfirmedNickname())->toBeFalse();

    $this->actingAs($user)->patchJson('/api/me', ['name' => 'ada.dois'])->assertOk();
});

it('o login pelo google vira apelido sem espaco, e desempata quando ja existe', function (): void {
    expect(User::freeNickname('Ada Byron'))->toBe('adabyron');

    User::factory()->create(['name' => 'adabyron']);

    expect(User::freeNickname('Ada Byron'))->toBe('adabyron2');
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
