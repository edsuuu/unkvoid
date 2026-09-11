<?php

declare(strict_types=1);

namespace App\Notifications;

use App\Models\User;
use Illuminate\Notifications\Messages\MailMessage;
use Illuminate\Notifications\Notification;

final class NewLoginNotification extends Notification
{
    public function __construct(
        private readonly string $via,
        private readonly string $ip,
        private readonly string $agent,
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
            ->subject('Novo acesso à sua conta do Unkvoid')
            ->view('emails.new-login', [
                'rows' => [
                    'Quando' => now()->timezone('America/Sao_Paulo')->format('d/m/Y H:i'),
                    'Por onde' => $this->via,
                    'Endereço IP' => $this->ip,
                    'Dispositivo' => mb_substr($this->agent, 0, 120),
                ],
                'resetUrl' => route('password.request'),
            ]);
    }
}
