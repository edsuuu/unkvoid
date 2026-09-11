<?php

declare(strict_types=1);

namespace App\Notifications;

use App\Models\User;
use Illuminate\Notifications\Messages\MailMessage;
use Illuminate\Notifications\Notification;
use Illuminate\Support\Facades\Config;

final class ResetPasswordNotification extends Notification
{
    public function __construct(
        private readonly string $token,
    ) {}

    /**
     * @return array<int, string>
     */
    public function via(User $notifiable): array
    {
        return ['mail'];
    }

    public function toMail(User $notifiable): MailMessage
    {
        return (new MailMessage)
            ->subject('Redefinir a senha do Unkvoid')
            ->view('emails.reset-password', [
                'url' => route('password.reset', ['token' => $this->token, 'email' => $notifiable->email]),
                'minutes' => Config::integer('auth.passwords.users.expire', 60),
            ]);
    }
}
