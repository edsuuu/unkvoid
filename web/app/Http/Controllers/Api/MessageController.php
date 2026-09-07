<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Http\Controllers\Controller;
use App\Http\Requests\Api\SendMessageRequest;
use App\Http\Resources\MessageResource;
use App\Models\Channel;
use App\Models\Message;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Illuminate\Support\Facades\Log;
use Symfony\Component\HttpKernel\Exception\AccessDeniedHttpException;
use Throwable;

final class MessageController extends Controller
{
    public function index(Request $request, Channel $channel): AnonymousResourceCollection
    {
        $this->ensureMember($request, $channel);

        return MessageResource::collection(
            $channel->messages()->with('user')->latest('created_at')->limit(60)->get()->reverse()->values()
        );
    }

    /**
     * @throws Throwable
     */
    public function store(SendMessageRequest $request, Channel $channel): MessageResource
    {
        $this->ensureMember($request, $channel);

        try {
            $message = Message::create([
                'channel_id' => $channel->id,
                'user_id' => $request->user()->id,
                'content' => $request->content(),
            ]);
        } catch (Throwable $exception) {
            Log::channel('servers')->error('[ERROR] failed to save the message through the API', ['exception' => $exception]);

            throw $exception;
        }

        return new MessageResource($message->load('user'));
    }

    private function ensureMember(Request $request, Channel $channel): void
    {
        if (! $channel->server->memberFor($request->user())) {
            throw new AccessDeniedHttpException(__('You are not a member of this server.'));
        }
    }
}
