<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Requests\Api\Friends\StoreDirectMessageRequest;
use App\Http\Requests\Api\Friends\UpdateDirectMessageRequest;
use App\Http\Requests\Api\Servers\IndexMessageRequest;
use App\Http\Resources\Api\ConversationResource;
use App\Http\Resources\Api\DirectMessageResource;
use App\Models\DirectMessage;
use App\Models\User;
use Illuminate\Container\Attributes\CurrentUser;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Http\Response;
use Throwable;

/**
 * Conversa direta entre duas pessoas: a lista, o fio, e o que se manda nele.
 */
final class DirectMessageController
{
    public function index(#[CurrentUser] User $user): AnonymousResourceCollection
    {
        return ConversationResource::collection(DirectMessage::conversationsFor($user));
    }

    /**
     * @throws Throwable
     */
    public function show(IndexMessageRequest $request, User $user, #[CurrentUser] User $viewer): AnonymousResourceCollection
    {
        $before = $request->filled('before') ? $request->integer('before') : null;

        return DirectMessageResource::collection(DirectMessage::conversation($viewer, $user, $before));
    }

    /**
     * @throws Throwable
     */
    public function read(User $user, #[CurrentUser] User $viewer): Response
    {
        DirectMessage::markRead($viewer, $user);

        return response()->noContent();
    }

    /**
     * @throws Throwable
     */
    public function store(StoreDirectMessageRequest $request, User $user, #[CurrentUser] User $sender): DirectMessageResource
    {
        return new DirectMessageResource(DirectMessage::send($sender, $user, $request->string('body')->toString()));
    }

    /**
     * @throws Throwable
     */
    public function update(UpdateDirectMessageRequest $request, DirectMessage $directMessage, #[CurrentUser] User $user): DirectMessageResource
    {
        $directMessage->edit($user, $request->string('body')->toString());

        return new DirectMessageResource($directMessage);
    }

    /**
     * @throws Throwable
     */
    public function destroy(DirectMessage $directMessage, #[CurrentUser] User $user): Response
    {
        $directMessage->remove($user);

        return response()->noContent();
    }
}
