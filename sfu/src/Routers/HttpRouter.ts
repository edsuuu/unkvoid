import { Router } from 'express';

import { BroadcastController } from '../Http/Controller/BroadcastController.js';
import { HealthController } from '../Http/Controller/HealthController.js';
import { RoomController } from '../Http/Controller/RoomController.js';
import { verifySignature } from '../Http/Middleware/VerifySignature.js';
import type { Broadcaster } from '../Services/Broadcaster.js';
import type { RoomRegistry } from '../Services/RoomRegistry.js';

export class HttpRouter {
    public static routes(registry: RoomRegistry, broadcaster: Broadcaster): Router {
        const health = new HealthController(registry);
        const rooms = new RoomController(registry);
        const broadcasts = new BroadcastController(broadcaster);
        const router = Router();

        router.get('/health', (request, response) => health.show(request, response));

        router.post('/rooms/:room/kick', verifySignature, (request, response) =>
            rooms.kick(request, response),
        );

        router.post('/rooms/:room/mute', verifySignature, (request, response, next) => {
            rooms.mute(request, response).catch(next);
        });

        router.post('/broadcast', verifySignature, (request, response) =>
            broadcasts.store(request, response),
        );

        router.get('/presence', verifySignature, (request, response) =>
            rooms.presence(request, response),
        );

        return router;
    }
}
