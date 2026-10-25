<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\FriendshipStatusEnum;
use App\Enums\PermissionEnum;
use App\Events\UserStatusUpdated;
use App\Models\Concerns\LogsFailedWrites;
use App\Notifications\NewLoginNotification;
use App\Notifications\ResetPasswordNotification;
use App\Services\Storage\BucketService;
use Database\Factories\UserFactory;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Attributes\Hidden;
use Illuminate\Database\Eloquent\Casts\Attribute;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Foundation\Auth\User as Authenticatable;
use Illuminate\Http\UploadedFile;
use Illuminate\Notifications\Notifiable;
use Illuminate\Notifications\Notification;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\Config;
use Illuminate\Support\Facades\Date;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Str;
use Laravel\Sanctum\HasApiTokens;
use Override;
use OwenIt\Auditing\Contracts\Auditable;
use Spatie\Permission\Models\Role;
use Spatie\Permission\Traits\HasRoles;
use Throwable;

#[Fillable(['name', 'email', 'password', 'google_id', 'avatar_url', 'avatar_id', 'email_verified_at', 'nickname_confirmed_at', 'bio', 'pronouns', 'banner_color', 'custom_status', 'status', 'status_until'])]
#[Hidden(['password', 'remember_token'])]
final class User extends Authenticatable implements Auditable
{
    use HasApiTokens;

    /** @use HasFactory<UserFactory> */
    use HasFactory;

    use HasRoles;
    use LogsFailedWrites;
    use Notifiable;
    use \OwenIt\Auditing\Auditable;

    public const string ROLE_ADMIN = 'Administrador';

    /** Depois disto o lugar volta a ser desconhecido, e o aviso sai de novo. */
    private const int KNOWN_LOGIN_DAYS = 60;

    /**
     * A foto vem sempre junto: o avatar aparece em toda lista de membro, mensagem, amigo e
     * conversa, e sem isso cada linha da lista viraria uma consulta.
     *
     * @var list<string>
     */
    protected $with = ['avatar'];

    /**
     * O id de dentro de um `user:<id>`.
     */
    public static function fromSubject(string $subject): int
    {
        return (int) mb_substr($subject, 5);
    }

    /**
     * Um apelido livre a partir de um texto qualquer — o nome que o Google manda, por
     * exemplo. Tira acento e espaço, corta no tamanho e acrescenta número enquanto o
     * apelido já for de alguém.
     */
    public static function freeNickname(string $seed): string
    {
        $base = Str::of($seed)->ascii()->lower()->replaceMatches('/[^a-z0-9._]+/', '')->limit(28, '')->toString();

        if (mb_strlen($base) < 3) {
            $base = 'pessoa';
        }

        $nickname = $base;

        for ($suffix = 2; self::query()->where('name', $nickname)->exists(); $suffix++) {
            $nickname = $base.$suffix;
        }

        return $nickname;
    }

    /**
     * A conta que nasce com o nome e a senha, pelo app: o apelido livre tirado do nome, já
     * confirmado, e é por ele que a conta entra (`loginColumn`). A coluna de e-mail não aceita
     * vazio, então ele é de mentira — `apelido@<uuid>.unkvoid.com`, único pelo uuid — até a
     * pessoa pôr um de verdade (`addEmail`). Nada sai para ele (`routeNotificationForMail`).
     *
     * @throws Throwable
     */
    public static function createFromName(string $name, string $password): self
    {
        $nickname = self::freeNickname($name);

        return self::query()->create([
            'name' => $nickname,
            'email' => $nickname.'@'.Str::uuid()->toString().'.unkvoid.com',
            'password' => $password,
            'nickname_confirmed_at' => now(),
        ]);
    }

    /**
     * Entra-se pelo e-mail ou pelo apelido, e o apelido não tem `@`.
     */
    public static function loginColumn(string $login): string
    {
        return str_contains($login, '@') ? 'email' : 'name';
    }

    /**
     * O e-mail ainda é o de mentira de quem entrou com o nome.
     */
    public function hasPlaceholderEmail(): bool
    {
        return (bool) preg_match('/@[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}\.unkvoid\.com$/', $this->email);
    }

    /**
     * O e-mail de verdade de quem entrou com o nome: daí em diante a conta também entra por
     * ele, e recebe os avisos e a troca de senha.
     *
     * @throws Throwable
     */
    public function addEmail(string $email): void
    {
        self::write('falha ao pôr o e-mail na conta', fn () => $this->update(['email' => $email]), ['user_id' => $this->id]);
    }

    /**
     * Para o e-mail de mentira não sai nada: nem boas-vindas, nem aviso de login, nem troca de
     * senha. O canal de e-mail do Laravel pula quem não tem endereço.
     */
    public function routeNotificationForMail(): ?string
    {
        return $this->hasPlaceholderEmail() ? null : $this->email;
    }

    /**
     * Troca o apelido, a qualquer hora. A primeira troca também confirma o automático, e a
     * data dela fica: é a da escolha, não a da última troca.
     *
     * @throws Throwable
     */
    public function changeNickname(string $name): void
    {
        self::write('falha ao trocar o apelido', fn () => $this->update([
            'name' => $name,
            'nickname_confirmed_at' => $this->nickname_confirmed_at ?? now(),
        ]), ['user_id' => $this->id]);
    }

    /**
     * O cartão de perfil. Só os campos que vieram mudam; `null` limpa.
     *
     * @param  array{bio?: ?string, pronouns?: ?string, banner_color?: ?string, custom_status?: ?string}  $profile
     *
     * @throws Throwable
     */
    public function changeProfile(array $profile): void
    {
        if ($profile === []) {
            return;
        }

        self::write('falha ao trocar o perfil', fn () => $this->update($profile), ['user_id' => $this->id]);
    }

    /**
     * O prazo é calculado aqui para que os clientes não possam escolher quando o status volta.
     *
     * @throws Throwable
     */
    public function changeStatus(string $status, ?int $minutes): void
    {
        $until = is_null($minutes) ? null : now()->addMinutes($minutes);

        self::write('falha ao trocar o status', fn () => $this->update([
            'status' => $status,
            'status_until' => $until,
        ]), ['user_id' => $this->id]);

        $this->refresh();
        self::publish(new UserStatusUpdated($this));
    }

    public function effectiveStatus(): string
    {
        $until = $this->getAttributes()['status_until'] ?? null;

        if (is_string($until) && Date::parse($until)->isPast()) {
            $this->expireStatusIfDue();

            return 'online';
        }

        $status = $this->getAttributes()['status'] ?? 'online';

        return is_string($status) ? $status : 'online';
    }

    public function publicStatus(): string
    {
        return $this->effectiveStatus() === 'invisible' ? 'offline' : $this->effectiveStatus();
    }

    /**
     * @throws Throwable
     */
    public function expireStatusIfDue(): void
    {
        $expired = self::write('falha ao encerrar status temporário', function (): ?self {
            $user = self::query()->whereKey($this->id)->whereNotNull('status_until')->where('status_until', '<=', now())->lockForUpdate()->first();

            if (is_null($user)) {
                return null;
            }

            $user->update(['status' => 'online', 'status_until' => null]);

            return $user;
        }, ['user_id' => $this->id]);

        if (! is_null($expired)) {
            $this->setRawAttributes($expired->getAttributes(), true);
            self::publish(new UserStatusUpdated($expired));
        } else {
            $this->refresh();
        }
    }

    public function hasConfirmedNickname(): bool
    {
        return ! is_null($this->nickname_confirmed_at);
    }

    /**
     * A foto que a pessoa manda vence a do Google. A antiga só sai depois que a conta já
     * aponta para a nova: falhar aqui deixa lixo no bucket, nunca um avatar quebrado na tela.
     *
     * @throws Throwable
     */
    public function setAvatar(UploadedFile $avatar, BucketService $bucket): void
    {
        $previous = $this->avatar;
        $file = File::put($this, $avatar, 'avatars', $bucket);

        self::write('falha ao guardar a foto de perfil', fn () => $this->update(['avatar_id' => $file->id]), ['user_id' => $this->id]);

        $this->setRelation('avatar', $file);

        $previous?->forget();
    }

    /**
     * @throws Throwable
     */
    public function removeAvatar(): void
    {
        $previous = $this->avatar;

        self::write('falha ao tirar a foto de perfil', fn () => $this->update(['avatar_id' => null]), ['user_id' => $this->id]);

        $this->setRelation('avatar', null);

        $previous?->forget();
    }

    /**
     * @return BelongsTo<File, $this>
     */
    public function avatar(): BelongsTo
    {
        return $this->belongsTo(File::class);
    }

    public function initials(): string
    {
        return Str::of($this->name)
            ->explode(' ')
            ->take(2)
            ->map(fn ($word) => Str::substr($word, 0, 1))
            ->implode('');
    }

    public function isAdmin(): bool
    {
        return $this->hasRole(self::ROLE_ADMIN);
    }

    /**
     * O `sub` que o SFU carrega: `user:<id>`.
     */
    public function subject(): string
    {
        return 'user:'.$this->id;
    }

    /**
     * Quem pode ouvir o quê no tempo real, que o SFU pergunta a cada inscrição. Canal
     * oculto é sobrescrita, não flag: sem `VIEW_CHANNEL` não se ouve nem o chat.
     */
    public function canSubscribe(string $channel): bool
    {
        [$type, $id] = array_pad(explode('.', $channel, 2), 2, '');

        if ($type === 'user') {
            return $this->id === (int) $id;
        }

        if ($type === 'friend') {
            return Friendship::query()
                ->where('status', FriendshipStatusEnum::Accepted)
                ->where(fn ($query) => $query->where('requester_id', $this->id)->where('addressee_id', (int) $id)
                    ->orWhere(fn ($pair) => $pair->where('requester_id', (int) $id)->where('addressee_id', $this->id)))
                ->exists();
        }

        if ($type === 'server') {
            $server = Server::query()->find((int) $id);

            return ! is_null($server) && ! is_null($server->memberOf($this));
        }

        if ($type !== 'channel') {
            return false;
        }

        $target = Channel::query()->find($id);

        if (is_null($target)) {
            return false;
        }

        $member = $target->server->memberOf($this);

        return ! is_null($member) && $member->can(PermissionEnum::ViewChannel, $target);
    }

    /**
     * E-mail nunca derruba um login ou um cadastro: se o servidor de e-mail estiver fora,
     * a pessoa entra do mesmo jeito e o problema fica no log.
     */
    /**
     * Avisa por e-mail que entraram na conta — mas só quando a entrada vem de um lugar que
     * ainda não conhecemos.
     *
     * Antes saía um e-mail a cada login, inclusive do mesmo computador e do mesmo IP de
     * sempre. Aviso que chega toda hora deixa de ser aviso: a pessoa passa a apagar sem ler,
     * e o dia em que alguém entrar de verdade vai passar batido junto.
     *
     * O que se guarda é o hash de IP + navegador, no cache, por 60 dias — não o IP. Assim
     * não é preciso coluna nova, e o que fica gravado não diz de onde ninguém acessou.
     */
    public function notifyNewLoginIfUnknown(string $source, string $ip, string $agent): void
    {
        $known = 'login:'.$this->id.':'.hash('sha256', $ip.'|'.$agent);

        // `add` grava e devolve `false` se a chave já existia: a pergunta e a marcação são
        // a mesma operação, e dois logins ao mesmo tempo não mandam dois e-mails.
        if (! Cache::add($known, true, self::KNOWN_LOGIN_DAYS * 86400)) {
            return;
        }

        $this->notifyQuietly(new NewLoginNotification($source, $ip, $agent));
    }

    public function notifyQuietly(Notification $notification): void
    {
        try {
            $this->notify($notification);
        } catch (Throwable $exception) {
            Log::channel('daily')->warning('[WARN] não deu para enviar o e-mail', [
                'notification' => $notification::class,
                'user_id' => $this->id,
                'message' => $exception->getMessage(),
            ]);
        }
    }

    #[Override]
    public function sendPasswordResetNotification($token): void
    {
        $this->notifyQuietly(new ResetPasswordNotification((string) $token));
    }

    #[Override]
    protected static function booted(): void
    {
        // Quem entra com o e-mail do dono do projeto já nasce administrador, sem seeder nem
        // senha: é o que liga o `admin` do `/api/me` e abre o log-viewer.
        self::created(function (User $user): void {
            $adminEmail = mb_strtolower(mb_trim(Config::string('unkvoid.admin_email')));

            if ($adminEmail === '' || mb_strtolower($user->email) !== $adminEmail) {
                return;
            }

            Role::query()->firstOrCreate(['name' => self::ROLE_ADMIN, 'guard_name' => 'web']);
            $user->assignRole(self::ROLE_ADMIN);
        });
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'email_verified_at' => 'datetime',
            'nickname_confirmed_at' => 'datetime',
            'status_until' => 'datetime',
            'password' => 'hashed',
        ];
    }

    /**
     * A foto enviada vence a do Google, que é o que está na coluna. A URL do bucket sai
     * assinada e vence.
     *
     * @return Attribute<?string, never>
     */
    protected function avatarUrl(): Attribute
    {
        return Attribute::get(fn (mixed $value): ?string => $this->avatar?->url() ?? (is_string($value) ? $value : null));
    }

    /**
     * @return Attribute<string, never>
     */
    protected function status(): Attribute
    {
        return Attribute::get(fn (): string => $this->effectiveStatus());
    }
}
