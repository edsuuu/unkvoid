import type { WebSocket } from 'ws';

import type { Subscriptions } from './Subscriptions.js';

export class Broadcaster {
    public constructor(private readonly subscriptions: Subscriptions) {}

    public send(channel: string, event: string, data: unknown, except?: WebSocket): number {
        const payload = JSON.stringify({ event, channel, data });
        let delivered = 0;

        for (const member of this.subscriptions.members(channel)) {
            if (member.socket === except || member.socket.readyState !== member.socket.OPEN) {
                continue;
            }

            member.socket.send(payload);
            delivered += 1;
        }

        return delivered;
    }
}
