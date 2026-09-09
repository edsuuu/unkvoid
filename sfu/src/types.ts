import type { WebSocket } from 'ws';

import type { Peer } from './Services/Peer.js';
import type { Room } from './Services/Room.js';

export type Session = {
    socket: WebSocket;
    room: Room | null;
    peer: Peer | null;
};

export type Resource = {
    toArray(): Record<string, unknown>;
};

export type PeerDescription = {
    peerId: string;
    name: string;
    producers: ProducerDescription[];
};

export type ProducerDescription = {
    producerId: string;
    kind: string;
    source: string;
};
