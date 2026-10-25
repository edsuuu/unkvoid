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
                // `mode=direct` abre o app da Store no Windows; fora dele, a página do produto.
                'WindowsStore' => 'https://apps.microsoft.com/detail/9NGPGTV3NPLW?mode=direct',
                'Linux' => $link(ReleasePlatformEnum::LinuxDeb),
            ],
            'aptUrl' => $aptUrl,
            'aptCommands' => implode("\n", [
                "curl -fsSL {$aptUrl}/unkvoid.gpg | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null",
                "echo \"deb [signed-by=/usr/share/keyrings/unkvoid.gpg] {$aptUrl} ./\" | sudo tee /etc/apt/sources.list.d/unkvoid.list",
                'sudo apt update && sudo apt install unkvoid',
            ]),
            'steps' => [
                ['number' => '01', 'title' => 'Crie um servidor', 'text' => 'Abra o app, entre na sua conta e crie um servidor para o seu grupo.', 'footLabel' => 'Organização', 'footValue' => 'canais e cargos'],
                ['number' => '02', 'title' => 'Convide as pessoas', 'text' => 'Mande o convite do servidor. Cada pessoa entra com a própria conta.', 'footLabel' => 'Controle', 'footValue' => 'permissões'],
                ['number' => '03', 'title' => 'Entre em voz e transmita', 'text' => 'No canal de voz, compartilhe a tela com todo mundo que está acompanhando.', 'footLabel' => 'Qualidade', 'footValue' => 'até 1440p60'],
            ],
            'reasons' => [
                ['number' => '01', 'title' => 'A CPU fica livre', 'text' => 'A codificação acontece no encoder de hardware da placa de vídeo.'],
                ['number' => '02', 'title' => 'Seu upload é sempre o mesmo', 'text' => 'Você envia um vídeo só. O servidor entrega esse vídeo para todo mundo da sala, sejam duas ou vinte pessoas.'],
                ['number' => '03', 'title' => 'Áudio do sistema, sem barra de navegador', 'text' => 'O som vai junto e nada fica por cima da tela transmitida.'],
            ],
            'roadmap' => [
                ['number' => '01', 'title' => 'Transmissão compartilhada', 'text' => 'Mais de uma pessoa transmitindo no mesmo canal de voz.'],
                ['number' => '02', 'title' => 'Câmera e áudio', 'text' => 'Compartilhar câmera e microfone junto com a tela.'],
                ['number' => '03', 'title' => 'Mais controles de servidor', 'text' => 'Ferramentas para organizar cargos, canais e permissões do grupo.'],
            ],
        ]);
    }
}
