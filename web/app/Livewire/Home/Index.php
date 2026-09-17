<?php

declare(strict_types=1);

namespace App\Livewire\Home;

use App\Enums\ReleasePlatformEnum;
use App\Models\Release;
use Illuminate\Support\Facades\Config;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Compartilhe sua tela em qualidade cheia')]
final class Index extends Component
{
    public function render(): View
    {
        $aptUrl = Config::string('unkvoid.apt_url');
        $latest = Release::latestPerPlatform();
        $newest = null;

        foreach ($latest as $release) {
            if (is_null($newest) || version_compare($release->version, $newest, '>')) {
                $newest = $release->version;
            }
        }

        $link = fn (ReleasePlatformEnum $platform): ?string => isset($latest[$platform->value]) ? route('downloads.platform', $platform->slug()) : null;

        return view('livewire.home.index', [
            'versionLabel' => is_null($newest) ? 'em breve' : 'v'.$newest,
            'links' => [
                'macOS' => $link(ReleasePlatformEnum::MacosDmg),
                'Windows' => $link(ReleasePlatformEnum::WindowsNsis) ?? $link(ReleasePlatformEnum::WindowsMsi),
                'WindowsExe' => $link(ReleasePlatformEnum::WindowsNsis),
                'WindowsMsi' => $link(ReleasePlatformEnum::WindowsMsi),
                'Linux' => $link(ReleasePlatformEnum::LinuxDeb),
            ],
            'aptUrl' => $aptUrl,
            'aptCommands' => implode("\n", [
                "curl -fsSL {$aptUrl}/unkvoid.gpg | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null",
                "echo \"deb [signed-by=/usr/share/keyrings/unkvoid.gpg] {$aptUrl} ./\" | sudo tee /etc/apt/sources.list.d/unkvoid.list",
                'sudo apt update && sudo apt install unkvoid',
            ]),
            'steps' => [
                ['number' => '01', 'title' => 'Crie uma sala', 'text' => 'Abra o app, escreva seu nome e crie uma sala. Sai um código de 12 caracteres.', 'footLabel' => 'Código da sala', 'footValue' => 'k3m9xq2vt7bd'],
                ['number' => '02', 'title' => 'Envie o código', 'text' => 'Quem colar o código entra. Sem conta, sem cadastro, sem link de reunião e sem chat.', 'footLabel' => 'O que é preciso', 'footValue' => 'só o código'],
                ['number' => '03', 'title' => 'Transmita', 'text' => 'Todo mundo vê a tela de quem estiver transmitindo. A sala deixa de existir quando a última pessoa sai.', 'footLabel' => 'Qualidade', 'footValue' => 'até 1440p60'],
            ],
            'reasons' => [
                ['number' => '01', 'title' => 'A CPU fica livre', 'text' => 'A codificação acontece no encoder de hardware da placa de vídeo.'],
                ['number' => '02', 'title' => 'Seu upload é sempre o mesmo', 'text' => 'Você envia um vídeo só. O servidor entrega esse vídeo para todo mundo da sala, sejam duas ou vinte pessoas.'],
                ['number' => '03', 'title' => 'Áudio do sistema, sem barra de navegador', 'text' => 'O som vai junto e nada fica por cima da tela transmitida.'],
            ],
            'roadmap' => [
                ['number' => '01', 'title' => 'Login opcional', 'text' => 'Só para quem quiser guardar nome e preferências. O app segue funcionando inteiro sem login, como é hoje.'],
                ['number' => '02', 'title' => 'Servidores e salas personalizadas', 'text' => 'Salas com nome próprio, no lugar de um código sorteado.'],
                ['number' => '03', 'title' => 'Transmissão compartilhada', 'text' => 'Mais de uma pessoa transmitindo na mesma sala.'],
                ['number' => '04', 'title' => 'Câmera e áudio', 'text' => 'Compartilhar câmera e microfone junto com a tela.'],
            ],
        ]);
    }
}
