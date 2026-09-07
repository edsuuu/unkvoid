<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api;

use App\Actions\Servers\CreateServer;
use App\Actions\Servers\JoinServerByInvite;
use App\Http\Controllers\Controller;
use App\Http\Resources\ServerResource;
use App\Models\Server;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;
use Symfony\Component\HttpKernel\Exception\AccessDeniedHttpException;
use Throwable;

final class ServerController extends Controller
{
    public function index(Request $request): AnonymousResourceCollection
    {
        return ServerResource::collection($request->user()->servers()->with('channels')->get());
    }

    public function show(Request $request, Server $server): ServerResource
    {
        $this->ensureMember($request, $server);

        return new ServerResource($server->load(['channels', 'members.user']));
    }

    /**
     * @throws Throwable
     */
    public function store(Request $request, CreateServer $createServer): ServerResource
    {
        $validated = $request->validate(['name' => ['required', 'string', 'min:2', 'max:60']]);

        $server = $createServer->handle($request->user(), $validated['name']);

        return new ServerResource($server->load('channels'));
    }

    /**
     * @throws Throwable
     */
    public function join(Request $request, string $code, JoinServerByInvite $joinServerByInvite): ServerResource
    {
        return new ServerResource($joinServerByInvite->handle($request->user(), $code)->load('channels'));
    }

    public function destroy(Request $request, Server $server): JsonResponse
    {
        if ($server->owner_id !== $request->user()->id) {
            throw new AccessDeniedHttpException(__('Só o dono exclui o servidor.'));
        }

        $server->delete();

        return response()->json(['status' => 'excluido']);
    }

    private function ensureMember(Request $request, Server $server): void
    {
        if (! $server->memberFor($request->user())) {
            throw new AccessDeniedHttpException(__('Você não é membro deste servidor.'));
        }
    }
}
