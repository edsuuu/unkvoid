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
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Throwable;

/**
 * Mensagens de um canal de texto.
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
    public function store(StoreMessageRequest $request, Channel $channel, #[CurrentUser] User $user): MessageResource
    {
        $replyToId = $request->filled('reply_to_id') ? $request->integer('reply_to_id') : null;

        return new MessageResource($channel->post($user, $request->string('body')->toString(), $replyToId));
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
