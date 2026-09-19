<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Servers\IndexMessageRequest;
use App\Http\Requests\Api\Servers\StoreMessageRequest;
use App\Http\Requests\Api\Servers\UpdateMessageRequest;
use App\Http\Resources\Api\MessageResource;
use App\Models\Channel;
use App\Models\Message;
use App\Models\User;
use App\Services\Storage\BucketService;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Illuminate\Http\UploadedFile;
use Throwable;

/**
 * Mensagens de um canal, de texto ou de voz: o canal de voz também tem chat.
 */
final class MessageController
{
    /**
     * @throws Throwable
     */
    public function index(IndexMessageRequest $request, Channel $channel, #[CurrentUser] User $user): AnonymousResourceCollection
    {
        $before = $request->filled('before') ? $request->integer('before') : null;

        return MessageResource::collection($channel->messagesBefore($user, $before));
    }

    /**
     * @throws Throwable
     */
    public function store(StoreMessageRequest $request, Channel $channel, #[CurrentUser] User $user, BucketService $bucket): MessageResource
    {
        $replyToId = $request->filled('reply_to_id') ? $request->integer('reply_to_id') : null;

        /** @var array<int, UploadedFile> $images */
        $images = $request->file('images', []);

        return new MessageResource($channel->post($user, $request->string('body')->toString(), $bucket, $replyToId, $images));
    }

    /**
     * @throws Throwable
     */
    public function update(UpdateMessageRequest $request, Message $message, #[CurrentUser] User $user): MessageResource
    {
        $message->edit($user, $request->string('body')->toString());

        return new MessageResource($message);
    }

    /**
     * @throws Throwable
     */
    public function destroy(Message $message, #[CurrentUser] User $user): Response
    {
        $message->remove($user);

        return response()->noContent();
    }
}
