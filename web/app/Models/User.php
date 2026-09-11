<?php

declare(strict_types=1);

namespace App\Models;

use App\Notifications\ResetPasswordNotification;
use Database\Factories\UserFactory;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Attributes\Hidden;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Foundation\Auth\User as Authenticatable;
use Illuminate\Notifications\Notifiable;
use Illuminate\Notifications\Notification;
use Illuminate\Support\Facades\Config;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Str;
use Laravel\Sanctum\HasApiTokens;
use Override;
use OwenIt\Auditing\Contracts\Auditable;
use Spatie\Permission\Models\Role;
use Spatie\Permission\Traits\HasRoles;
use Throwable;

#[Fillable(['name', 'email', 'password', 'google_id', 'avatar_url', 'email_verified_at'])]
#[Hidden(['password', 'remember_token'])]
final class User extends Authenticatable implements Auditable
{
    use HasApiTokens;

    /** @use HasFactory<UserFactory> */
    use HasFactory;

    use HasRoles;
    use Notifiable;
    use \OwenIt\Auditing\Auditable;

    public const string ROLE_ADMIN = 'Administrador';

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
     * E-mail nunca derruba um login ou um cadastro: se o servidor de e-mail estiver fora,
     * a pessoa entra do mesmo jeito e o problema fica no log.
     */
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
        // Quem entra com o e-mail do dono do projeto já nasce administrador. É o que
        // permite abrir o painel na primeira vez, sem seeder nem senha.
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
            'password' => 'hashed',
        ];
    }
}
