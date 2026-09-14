<?php

declare(strict_types=1);

use App\Models\Server;
use App\Models\ServerMember;
use App\Models\ServerRole;
use App\Models\User;
use Illuminate\Foundation\Testing\RefreshDatabase;
use Illuminate\Support\Facades\Http;
use Tests\TestCase;

pest()->extend(TestCase::class)
    ->use(RefreshDatabase::class)
    ->in('Feature');

function joinServer(Server $server, User $user): ServerMember
{
    return $server->members()->create(['user_id' => $user->id, 'joined_at' => now()]);
}

function giveRole(Server $server, User $user, int $permissions, int $position = 1): ServerRole
{
    $role = $server->roles()->create(['name' => 'Cargo '.$position, 'position' => $position, 'permissions' => $permissions]);
    $server->members()->where('user_id', $user->id)->firstOrFail()->roles()->attach($role);

    return $role;
}

function fakeSfu(string $room = '', User ...$users): void
{
    $peers = [];

    foreach ($users as $user) {
        $peers[] = ['sub' => "user:{$user->id}", 'name' => $user->name, 'sources' => ['mic']];
    }

    Http::fake([
        '*/presence' => Http::response(['rooms' => $room === '' ? [] : [$room => $peers]]),
        '*' => Http::response(['kicked' => 1, 'muted' => 1]),
    ]);
}

/**
 * @param  array<string, mixed>  $body
 * @return array<string, string>
 */
function sfuHeaders(array $body, string $path = '/api/sfu/events', ?string $secret = null): array
{
    $timestamp = (string) time();
    $json = json_encode($body, JSON_THROW_ON_ERROR);

    return [
        'X-Unkvoid-Timestamp' => $timestamp,
        'X-Unkvoid-Signature' => hash_hmac('sha256', "{$timestamp}\nPOST\n{$path}\n{$json}", $secret ?? config('services.sfu.secret')),
    ];
}
