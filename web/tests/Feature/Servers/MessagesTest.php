<?php

declare(strict_types=1);

use App\Enums\MessageTypeEnum;
use App\Enums\PermissionEnum;
use App\Events\MessageDeleted;
use App\Events\MessageSent;
use App\Events\MessageUpdated;
use App\Models\Channel;
use App\Models\File;
use App\Models\Message;
use App\Models\Server;
use App\Models\User;
use Aws\CommandInterface;
use Aws\Result;
use Aws\S3\Exception\S3Exception;
use GuzzleHttp\Promise\Create;
use GuzzleHttp\Promise\PromiseInterface;
use GuzzleHttp\Psr7\Response;
use Illuminate\Http\UploadedFile;
use Illuminate\Log\Events\MessageLogged;
use Illuminate\Support\Facades\Config;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Storage;
use OwenIt\Auditing\Models\Audit;

beforeEach(function (): void {
    Http::fake();
});

it('manda, edita e apaga mensagem com as regras do Discord', function (): void {
    Event::fake([MessageSent::class, MessageUpdated::class, MessageDeleted::class]);

    $owner = User::factory()->create();
    $author = User::factory()->create();
    $other = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    joinServer($server, $other);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $messageId = $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi'])
        ->assertCreated()
        ->assertJsonPath('data.body', 'oi')
        ->assertJsonPath('data.user.id', $author->id)
        ->assertJsonPath('data.channel_id', $channel->id)
        ->json('data.id');

    Event::assertDispatched(MessageSent::class, fn (MessageSent $event): bool => $event->message->id === $messageId && $event->broadcastOn()->name === "private-channel.{$channel->id}");

    $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => ''])->assertUnprocessable();
    $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => str_repeat('a', 2001)])->assertUnprocessable();

    $this->actingAs($other, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'editado'])->assertForbidden();
    $this->actingAs($owner, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'editado'])->assertForbidden();
    $this->actingAs($author, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'editado'])->assertOk()->assertJsonPath('data.body', 'editado');

    Event::assertDispatched(MessageUpdated::class);

    $this->actingAs($other, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertForbidden();
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();

    Event::assertDispatched(MessageDeleted::class, fn (MessageDeleted $event): bool => $event->id === $messageId && $event->channelId === $channel->id);

    // Apagar virou soft delete: a mensagem sai da conversa e continua no banco, que é de
    // onde a auditoria lê o que foi dito.
    $this->assertSoftDeleted('messages', ['id' => $messageId]);
    expect(Message::query()->count())->toBe(0);
});

it('quem tem MANAGE_MESSAGES apaga a mensagem dos outros, quem não tem SEND_MESSAGES não escreve', function (): void {
    $owner = User::factory()->create();
    $author = User::factory()->create();
    $moderator = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    joinServer($server, $moderator);
    giveRole($server, $moderator, PermissionEnum::ManageMessages->value);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $messageId = $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi'])->assertCreated()->json('data.id');

    $this->actingAs($moderator, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();

    $channel->overwrites()->create(['target_type' => 'member', 'target_id' => $author->id, 'allow' => 0, 'deny' => PermissionEnum::SendMessages->value]);

    $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'de novo'])->assertForbidden();
});

it('a auditoria registra quem editou e quem apagou a mensagem, e guarda o corpo anterior', function (): void {
    config(['audit.console' => true]);
    $owner = User::factory()->create();
    $author = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $messageId = $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi'])->assertCreated()->json('data.id');

    $this->actingAs($author, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'editado'])->assertOk();

    $edited = Audit::query()->where('event', 'updated')->where('auditable_type', Message::class)->where('auditable_id', $messageId)->firstOrFail();

    expect($edited->user_id)->toBe($author->id)
        ->and($edited->old_values['body'])->toBe('oi')
        ->and($edited->new_values['body'])->toBe('editado');

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();

    $deleted = Audit::query()->where('event', 'deleted')->where('auditable_type', Message::class)->where('auditable_id', $messageId)->firstOrFail();

    expect($deleted->user_id)->toBe($owner->id)
        ->and($deleted->old_values['body'])->toBe('editado');
});

it('lista as 50 mais recentes antes de um id, em ordem crescente', function (): void {
    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = Channel::query()->where('server_id', $server->id)->where('type', 'text')->firstOrFail();

    for ($index = 1; $index <= 60; $index++) {
        $channel->messages()->create(['user_id' => $owner->id, 'body' => "mensagem {$index}"]);
    }

    $page = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');

    expect($page)->toHaveCount(50)
        ->and($page[0]['body'])->toBe('mensagem 11')
        ->and($page[49]['body'])->toBe('mensagem 60');

    $older = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages?before={$page[0]['id']}")->assertOk()->json('data');

    expect($older)->toHaveCount(10)
        ->and($older[0]['body'])->toBe('mensagem 1');

    $this->actingAs(User::factory()->create(), 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertForbidden();
});

it('responder guarda a mensagem original, e so aceita mensagem do mesmo canal', function (): void {
    Event::fake([MessageSent::class]);

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = $server->channels()->where('type', 'text')->firstOrFail();
    $other = Channel::query()->create(['server_id' => $server->id, 'name' => 'outro', 'type' => 'text', 'position' => 9]);

    $original = $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'quem vem hoje?'])
        ->assertCreated()
        ->json('data.id');

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'eu vou', 'reply_to_id' => $original])
        ->assertCreated()
        ->assertJsonPath('data.reply_to.id', $original)
        ->assertJsonPath('data.reply_to.name', $owner->name)
        ->assertJsonPath('data.reply_to.body', 'quem vem hoje?');

    // Responder mensagem de outro canal vazaria o texto de um canal que a pessoa pode nem ver.
    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$other->id}/messages", ['body' => 'de outro canal', 'reply_to_id' => $original])
        ->assertStatus(422);

    $history = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');

    expect(collect($history)->firstWhere('body', 'eu vou')['reply_to']['body'])->toBe('quem vem hoje?');

    // Apagar a original não leva a resposta junto: o fio continua de pé, só a citação some.
    // Apagar aqui é soft delete, então a coluna segue apontando e é a relação que não resolve.
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$original}")->assertNoContent();

    $depois = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');
    $resposta = collect($depois)->firstWhere('body', 'eu vou');

    expect($resposta)->not->toBeNull()
        ->and($resposta['reply_to'])->toBeNull();
});

it('entrar por convite avisa no primeiro canal de texto, uma vez só', function (): void {
    Event::fake([MessageSent::class]);

    $owner = User::factory()->create();
    $guest = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")->assertOk();

    $notice = Message::query()->where('channel_id', $channel->id)->firstOrFail();

    expect($notice->type)->toBe(MessageTypeEnum::Join)
        ->and($notice->user_id)->toBe($guest->id)
        // O corpo é para o app antigo, que não conhece `type`: sem ele, balão vazio.
        ->and($notice->body)->toBe('chegou no servidor!');

    Event::assertDispatched(MessageSent::class, fn (MessageSent $event): bool => $event->message->id === $notice->id && $event->broadcastOn()->name === "private-channel.{$channel->id}");

    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")->assertOk();

    expect(Message::query()->count())->toBe(1);

    $chat = $this->actingAs($guest, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');

    expect($chat)->toHaveCount(1)
        ->and($chat[0]['type'])->toBe('join')
        ->and($chat[0]['user']['id'])->toBe($guest->id);
});

it('o aviso pula o canal escondido e a mensagem de gente continua sendo user', function (): void {
    $owner = User::factory()->create();
    $guest = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $hidden = $server->channels()->where('type', 'text')->firstOrFail();
    $open = $server->channels()->create(['name' => 'avisos', 'type' => 'text', 'position' => 5]);

    $hidden->overwrites()->create([
        'target_type' => 'role',
        'target_id' => $server->everyoneRole()->id,
        'allow' => 0,
        'deny' => PermissionEnum::ViewChannel->value,
    ]);

    $this->actingAs($guest, 'sanctum')->postJson("/api/invites/{$server->invite_code}")->assertOk();

    expect(Message::query()->where('channel_id', $hidden->id)->count())->toBe(0)
        ->and(Message::query()->where('channel_id', $open->id)->where('type', 'join')->count())->toBe(1);

    $this->actingAs($guest, 'sanctum')->postJson("/api/channels/{$open->id}/messages", ['body' => 'oi'])
        ->assertCreated()
        ->assertJsonPath('data.type', 'user');
});

it('manda texto e imagens por multipart, e as imagens vão para `files`, para a resposta e para o MessageSent', function (): void {
    Storage::fake('s3');
    Event::fake([MessageSent::class]);

    $owner = User::factory()->create();
    $author = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    // `post`, e não `postJson`: no multipart de verdade todo campo chega como texto.
    $messageId = $this->actingAs($author, 'sanctum')
        ->post("/api/channels/{$channel->id}/messages", [
            'body' => 'olha isso',
            'images' => [UploadedFile::fake()->image('um.jpg'), UploadedFile::fake()->image('dois.png')],
        ], ['Accept' => 'application/json'])
        ->assertCreated()
        ->assertJsonPath('data.body', 'olha isso')
        ->assertJsonCount(2, 'data.files')
        ->assertJsonPath('data.files.0.mime_type', 'image/jpeg')
        ->assertJsonPath('data.files.1.mime_type', 'image/png')
        ->assertJsonPath('data.files.0.size', fn (int $size): bool => $size > 0)
        ->assertJsonPath('data.files.0.url', fn (string $url): bool => str_contains($url, "messages/{$author->id}/"))
        ->json('data.id');

    $files = Message::query()->findOrFail($messageId)->files;

    expect($files)->toHaveCount(2)
        ->and(File::query()->count())->toBe(2)
        ->and($files[0]->user_id)->toBe($author->id)
        ->and($files[0]->path)->toStartWith("messages/{$author->id}/");

    Storage::disk('s3')->assertExists([$files[0]->path, $files[1]->path]);

    Event::assertDispatched(MessageSent::class, function (MessageSent $event) use ($files): bool {
        $sent = $event->broadcastWith()['message']['files'];

        return count($sent) === 2 && $sent[0]['id'] === $files[0]->id && $sent[1]['id'] === $files[1]->id && array_keys($sent[0]) === ['id', 'url', 'mime_type', 'size'];
    });
});

it('manda mensagem só com imagem, e sem imagem o corpo continua obrigatório e `files` vem vazio', function (): void {
    Storage::fake('s3');

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['images' => [UploadedFile::fake()->image('animada.gif'), UploadedFile::fake()->image('leve.webp')]])
        ->assertCreated()
        ->assertJsonPath('data.body', '')
        ->assertJsonPath('data.files.0.mime_type', 'image/gif')
        ->assertJsonPath('data.files.1.mime_type', 'image/webp');

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'só texto'])
        ->assertCreated()
        ->assertJsonPath('data.files', []);

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", [])->assertUnprocessable()->assertJsonValidationErrors('body');
    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => '', 'images' => []])->assertUnprocessable()->assertJsonValidationErrors('body');
    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['body' => str_repeat('a', 2001), 'images' => [UploadedFile::fake()->image('um.jpg')]])
        ->assertUnprocessable()
        ->assertJsonValidationErrors('body');

    expect(Message::query()->count())->toBe(2);
});

it('recusa quatro imagens, imagem acima de 2 MB e o que não é imagem, sem guardar nada', function (): void {
    Storage::fake('s3');

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $four = [UploadedFile::fake()->image('1.jpg'), UploadedFile::fake()->image('2.jpg'), UploadedFile::fake()->image('3.jpg'), UploadedFile::fake()->image('4.jpg')];

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi', 'images' => $four])
        ->assertUnprocessable()
        ->assertJsonValidationErrors('images');

    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi', 'images' => [UploadedFile::fake()->image('boa.jpg'), UploadedFile::fake()->create('enorme.jpg', 2100, 'image/jpeg')]])
        ->assertUnprocessable()
        ->assertJsonValidationErrors('images.1');

    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi', 'images' => [UploadedFile::fake()->create('livro.pdf', 10, 'application/pdf')]])
        ->assertUnprocessable()
        ->assertJsonValidationErrors('images.0');

    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['body' => 'oi', 'images' => 'não é lista'])
        ->assertUnprocessable()
        ->assertJsonValidationErrors('images');

    expect(Message::query()->count())->toBe(0)
        ->and(File::query()->count())->toBe(0)
        ->and(Storage::disk('s3')->allFiles())->toBe([]);
});

it('sem SEND_MESSAGES, de fora do servidor ou em canal oculto a imagem não sobe nem aparece', function (): void {
    Storage::fake('s3');

    $owner = User::factory()->create();
    $member = User::factory()->create();
    $outsider = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    Server::createFor($outsider, 'Outra casa');
    joinServer($server, $member);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();
    $everyone = $server->roles()->where('is_everyone', true)->firstOrFail();

    $channel->overwrites()->create(['target_type' => 'member', 'target_id' => $member->id, 'allow' => 0, 'deny' => PermissionEnum::SendMessages->value]);

    $this->actingAs($member, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['images' => [UploadedFile::fake()->image('um.jpg')]])
        ->assertForbidden();
    $this->actingAs($outsider, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['images' => [UploadedFile::fake()->image('um.jpg')]])
        ->assertForbidden();

    // A recusa vem antes do upload: nada pode ter chegado ao bucket.
    expect(Storage::disk('s3')->allFiles())->toBe([])
        ->and(File::query()->count())->toBe(0);

    $messageId = $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['images' => [UploadedFile::fake()->image('segredo.jpg')]])
        ->assertCreated()
        ->json('data.id');

    $channel->overwrites()->where('target_type', 'member')->delete();
    $channel->overwrites()->create(['target_type' => 'role', 'target_id' => $everyone->id, 'allow' => 0, 'deny' => PermissionEnum::ViewChannel->value]);

    $this->actingAs($member, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertForbidden();
    $this->actingAs($member, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['images' => [UploadedFile::fake()->image('um.jpg')]])
        ->assertForbidden();
    $this->actingAs($member, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertForbidden();
    $this->actingAs($outsider, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertForbidden();

    expect(File::query()->count())->toBe(1);
});

it('o index devolve os `files` de cada mensagem numa consulta só', function (): void {
    Storage::fake('s3');

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    foreach (['um', 'dois', 'tres'] as $name) {
        $this->actingAs($owner, 'sanctum')
            ->postJson("/api/channels/{$channel->id}/messages", ['body' => $name, 'images' => [UploadedFile::fake()->image($name.'.jpg')]])
            ->assertCreated();
    }

    $this->actingAs($owner, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'sem imagem'])->assertCreated();

    DB::enableQueryLog();

    $page = $this->actingAs($owner, 'sanctum')->getJson("/api/channels/{$channel->id}/messages")->assertOk()->json('data');

    $fileQueries = array_filter(DB::getQueryLog(), fn (array $query): bool => str_contains((string) $query['query'], 'message_files'));

    expect($page)->toHaveCount(4)
        ->and($page[0]['files'])->toHaveCount(1)
        ->and($page[0]['files'][0])->toHaveKeys(['id', 'url', 'mime_type', 'size'])
        ->and($page[2]['files'][0]['id'])->not->toBe($page[0]['files'][0]['id'])
        ->and($page[3]['files'])->toBe([])
        ->and($fileQueries)->toHaveCount(1);
});

it('editar para corpo vazio só vale em mensagem com imagem, e o MessageUpdated leva os `files`', function (): void {
    Storage::fake('s3');
    Event::fake([MessageUpdated::class]);

    $owner = User::factory()->create();
    $author = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $withImage = $this->actingAs($author, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['body' => 'legenda', 'images' => [UploadedFile::fake()->image('um.jpg')]])
        ->assertCreated()
        ->json('data');
    $textOnly = $this->actingAs($author, 'sanctum')->postJson("/api/channels/{$channel->id}/messages", ['body' => 'só texto'])->assertCreated()->json('data.id');

    // Quem não é o autor leva 403, e não um 422 que contaria se a mensagem tem imagem.
    $this->actingAs($owner, 'sanctum')->patchJson("/api/messages/{$textOnly}", ['body' => ''])->assertForbidden();

    $this->actingAs($author, 'sanctum')->patchJson("/api/messages/{$textOnly}", ['body' => ''])->assertUnprocessable()->assertJsonValidationErrors('body');
    $this->actingAs($author, 'sanctum')->patchJson("/api/messages/{$withImage['id']}", [])->assertUnprocessable()->assertJsonValidationErrors('body');

    Event::assertNotDispatched(MessageUpdated::class);

    $this->actingAs($author, 'sanctum')->patchJson("/api/messages/{$withImage['id']}", ['body' => ''])
        ->assertOk()
        ->assertJsonPath('data.body', '')
        ->assertJsonCount(1, 'data.files')
        ->assertJsonPath('data.files.0.id', $withImage['files'][0]['id']);

    Event::assertDispatched(MessageUpdated::class, fn (MessageUpdated $event): bool => $event->broadcastWith()['message']['files'][0]['id'] === $withImage['files'][0]['id']);

    expect(Message::query()->findOrFail($textOnly)->body)->toBe('só texto')
        ->and(File::query()->count())->toBe(1);
});

it('apagar a mensagem tira as imagens do bucket, de `files` e da pivô', function (): void {
    Storage::fake('s3');

    $owner = User::factory()->create();
    $author = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $author);
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $kept = $this->actingAs($author, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['images' => [UploadedFile::fake()->image('fica.jpg')]])
        ->assertCreated()
        ->json('data.id');
    $removed = $this->actingAs($author, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['body' => 'mandei errado', 'images' => [UploadedFile::fake()->image('um.jpg'), UploadedFile::fake()->image('dois.png')]])
        ->assertCreated()
        ->json('data.id');

    $paths = Message::query()->findOrFail($removed)->files->pluck('path')->all();
    $keptPath = Message::query()->findOrFail($kept)->files->sole()->path;

    Storage::disk('s3')->assertExists($paths);

    // Quem apaga aqui é a moderação (o dono), não o autor: as imagens saem do mesmo jeito.
    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$removed}")->assertNoContent();

    $this->assertSoftDeleted('messages', ['id' => $removed]);
    Storage::disk('s3')->assertMissing($paths);
    Storage::disk('s3')->assertExists($keptPath);

    expect(File::query()->pluck('path')->all())->toBe([$keptPath])
        ->and(DB::table('message_files')->where('message_id', $removed)->count())->toBe(0)
        ->and(DB::table('message_files')->where('message_id', $kept)->count())->toBe(1);
});

it('o bucket fora do ar não segura a exclusão da mensagem, e a mensagem que não nasce não deixa imagem para trás', function (): void {
    Storage::fake('s3');

    $owner = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    $channel = $server->channels()->where('type', 'text')->firstOrFail();

    $messageId = $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['images' => [UploadedFile::fake()->image('um.jpg')]])
        ->assertCreated()
        ->json('data.id');

    // Daqui em diante o disco é um S3 que recusa apagar, e que estoura em vez de calar.
    Config::set('filesystems.disks.s3.throw', true);
    Config::set('filesystems.disks.s3.handler', fn (CommandInterface $command): PromiseInterface => str_starts_with($command->getName(), 'DeleteObject')
        ? Create::rejectionFor(new S3Exception('fora do ar', $command, ['response' => new Response(503)]))
        : Create::promiseFor(new Result([])));
    Storage::forgetDisk('s3');

    $logged = [];
    Event::listen(function (MessageLogged $event) use (&$logged): void {
        $logged[] = $event->message;
    });

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();

    $this->assertSoftDeleted('messages', ['id' => $messageId]);
    expect(File::query()->count())->toBe(0)
        ->and($logged)->toBe(['[ERRO] falha ao apagar o arquivo do bucket']);

    Storage::fake('s3');
    Message::creating(function (): never {
        throw new RuntimeException('o banco caiu depois do upload');
    });

    $this->actingAs($owner, 'sanctum')
        ->postJson("/api/channels/{$channel->id}/messages", ['images' => [UploadedFile::fake()->image('orfa.jpg')]])
        ->assertServerError();

    expect(File::query()->count())->toBe(0)
        ->and(Storage::disk('s3')->allFiles())->toBe([])
        ->and($logged)->toContain('[ERRO] falha ao enviar a mensagem');
});

it('canal de voz também tem chat, com as mesmas permissões do de texto', function (): void {
    Storage::fake('s3');
    Event::fake([MessageSent::class, MessageUpdated::class, MessageDeleted::class]);

    $owner = User::factory()->create();
    $member = User::factory()->create();
    $muted = User::factory()->create();
    $outsider = User::factory()->create();
    $server = Server::createFor($owner, 'Casa');
    joinServer($server, $member);
    joinServer($server, $muted);
    $voice = $server->channels()->where('type', 'voice')->firstOrFail();
    $everyone = $server->roles()->where('is_everyone', true)->firstOrFail();

    $voice->overwrites()->create(['target_type' => 'member', 'target_id' => $muted->id, 'allow' => 0, 'deny' => PermissionEnum::SendMessages->value]);

    $messageId = $this->actingAs($member, 'sanctum')
        ->postJson("/api/channels/{$voice->id}/messages", ['body' => 'alguém me ouve?', 'images' => [UploadedFile::fake()->image('tela.png')]])
        ->assertCreated()
        ->assertJsonPath('data.channel_id', $voice->id)
        ->assertJsonCount(1, 'data.files')
        ->json('data.id');

    Event::assertDispatched(MessageSent::class, fn (MessageSent $event): bool => $event->broadcastOn()->name === "private-channel.{$voice->id}");

    $this->actingAs($muted, 'sanctum')->postJson("/api/channels/{$voice->id}/messages", ['body' => 'eu não'])->assertForbidden();
    $this->actingAs($outsider, 'sanctum')->postJson("/api/channels/{$voice->id}/messages", ['body' => 'nem eu'])->assertForbidden();
    $this->actingAs($outsider, 'sanctum')->getJson("/api/channels/{$voice->id}/messages")->assertForbidden();

    // Sem SEND_MESSAGES ainda se lê: ler é VIEW_CHANNEL.
    $this->actingAs($muted, 'sanctum')->getJson("/api/channels/{$voice->id}/messages")
        ->assertOk()
        ->assertJsonCount(1, 'data')
        ->assertJsonPath('data.0.body', 'alguém me ouve?');

    $this->actingAs($member, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'agora sim'])->assertOk()->assertJsonPath('data.body', 'agora sim');
    Event::assertDispatched(MessageUpdated::class);

    $voice->overwrites()->create(['target_type' => 'role', 'target_id' => $everyone->id, 'allow' => 0, 'deny' => PermissionEnum::ViewChannel->value]);

    $this->actingAs($member, 'sanctum')->getJson("/api/channels/{$voice->id}/messages")->assertForbidden();
    $this->actingAs($member, 'sanctum')->postJson("/api/channels/{$voice->id}/messages", ['body' => 'sumiu'])->assertForbidden();
    $this->actingAs($member, 'sanctum')->patchJson("/api/messages/{$messageId}", ['body' => 'não vejo mais'])->assertForbidden();

    $this->actingAs($owner, 'sanctum')->deleteJson("/api/messages/{$messageId}")->assertNoContent();
    Event::assertDispatched(MessageDeleted::class, fn (MessageDeleted $event): bool => $event->channelId === $voice->id);

    expect(File::query()->count())->toBe(0);
});
