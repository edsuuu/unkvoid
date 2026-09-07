import type { WebSocket } from 'ws';

import type { RoleName } from './Enums/Role.js';
import type { Peer } from './Services/Peer.js';
import type { Room } from './Services/Room.js';

export type Session = {
    socket: WebSocket;
    room: Room | null;
    peer: Peer | null;
    watching: string | null;
};

export type TokenClaims = {
    sub: string;
    room: string;
    server?: string;
    role: RoleName;
    name?: string;
    avatar?: string | null;
    exp: number;
    iat?: number;
};

export type Resource = {
    toArray(): Record<string, unknown>;
};

export type PeerDescription = {
    peerId: string;
    name: string;
    avatar: string | null;
    role: RoleName;
    producers: ProducerDescription[];
};

export type ProducerDescription = {
    producerId: string;
    kind: string;
    source: string;
};
