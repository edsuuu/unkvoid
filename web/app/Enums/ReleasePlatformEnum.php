<?php

declare(strict_types=1);

namespace App\Enums;

/**
 * Os valores são as chaves que o atualizador do Tauri procura no `latest.json`. O
 * `.dmg` é o único que não entra lá: é só para quem instala pela primeira vez.
 */
enum ReleasePlatformEnum: string
{
    case WindowsNsis = 'windows-x86_64-nsis';
    case WindowsMsi = 'windows-x86_64-msi';
    case MacosApp = 'darwin-aarch64';
    case MacosDmg = 'darwin-aarch64-dmg';
    case LinuxDeb = 'linux-x86_64-deb';

    public static function fromSlug(string $slug): ?self
    {
        foreach (self::cases() as $platform) {
            if ($platform->slug() === $slug) {
                return $platform;
            }
        }

        return null;
    }

    public function label(): string
    {
        return match ($this) {
            self::WindowsNsis => 'Windows (.exe)',
            self::WindowsMsi => 'Windows (.msi)',
            self::MacosApp => 'macOS (atualizador .app.tar.gz)',
            self::MacosDmg => 'macOS (.dmg)',
            self::LinuxDeb => 'Linux (.deb)',
        };
    }

    public function slug(): string
    {
        return match ($this) {
            self::WindowsNsis => 'windows',
            self::WindowsMsi => 'windows-msi',
            self::MacosApp => 'macos-app',
            self::MacosDmg => 'macos',
            self::LinuxDeb => 'linux',
        };
    }

    public function updates(): bool
    {
        return $this !== self::MacosDmg;
    }
}
