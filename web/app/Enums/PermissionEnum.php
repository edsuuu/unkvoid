<?php

declare(strict_types=1);

namespace App\Enums;

/**
 * Os bits de docs/CONTRATO.md. Mudou aqui, muda no app.
 */
enum PermissionEnum: int
{
    case Administrator = 1 << 0;
    case ManageServer = 1 << 1;
    case ManageRoles = 1 << 2;
    case ManageChannels = 1 << 3;
    case KickMembers = 1 << 4;
    case BanMembers = 1 << 5;
    case CreateInvite = 1 << 6;
    case ViewAuditLog = 1 << 7;
    case ViewChannel = 1 << 8;
    case SendMessages = 1 << 9;
    case ManageMessages = 1 << 10;
    case Connect = 1 << 11;
    case Speak = 1 << 12;
    case Stream = 1 << 13;
    case Video = 1 << 14;
    case MuteMembers = 1 << 15;
    case DeafenMembers = 1 << 16;
    case MoveMembers = 1 << 17;

    public static function everyoneDefault(): int
    {
        return self::ViewChannel->value
            | self::SendMessages->value
            | self::Connect->value
            | self::Speak->value
            | self::Stream->value
            | self::Video->value
            | self::CreateInvite->value;
    }

    public static function all(): int
    {
        $bits = 0;

        foreach (self::cases() as $permission) {
            $bits |= $permission->value;
        }

        return $bits;
    }
}
