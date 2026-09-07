import type { WebSocket } from 'ws';

type Watcher = { socket: WebSocket; serverId: string };

type Membro = { name: string; avatar: string | null; joinedAt: number; sharing: boolean; screenProducerId: string | null };

type ChannelPresence = {
    members: ({ peerId: string } & Membro)[];
    startedAt: number;
};

/**
 * Quem está em qual canal de voz, empurrado por WebSocket para todo mundo do
 * servidor — inclusive quem não entrou em canal nenhum. Sem isto só dava para
 * saber quem está na sala entrando nela.
 */
export class PresenceRegistry {
    private readonly watchers = new Map<string, Set<Watcher>>();

    private readonly channelServer = new Map<string, string>();

    private readonly channels = new Map<string, Map<string, Membro>>();

    link(channelId: string, serverId: string | undefined): void {
        if (serverId) {
            this.channelServer.set(channelId, serverId);
        }
    }

    watch(socket: WebSocket, serverId: string): Watcher {
        const watcher: Watcher = { socket, serverId };
        const set = this.watchers.get(serverId) ?? new Set<Watcher>();

        set.add(watcher);
        this.watchers.set(serverId, set);

        return watcher;
    }

    unwatch(socket: WebSocket, serverId: string): void {
        const set = this.watchers.get(serverId);

        if (!set) {
            return;
        }

        for (const watcher of set) {
            if (watcher.socket === socket) {
                set.delete(watcher);
            }
        }

        if (set.size === 0) {
            this.watchers.delete(serverId);
        }
    }

    enter(channelId: string, peerId: string, name: string, avatar: string | null): void {
        const members = this.channels.get(channelId) ?? new Map();

        // Preserva o horário de entrada numa reconexão: o cronômetro que os outros
        // veem não pode zerar porque a sinalização caiu.
        const joinedAt = members.get(peerId)?.joinedAt ?? Date.now();

        const anterior = members.get(peerId);

        members.set(peerId, {
            name,
            avatar,
            joinedAt,
            sharing: anterior?.sharing ?? false,
            screenProducerId: anterior?.screenProducerId ?? null,
        });
        this.channels.set(channelId, members);
        this.publish(channelId);
    }

    /** Quem não está no canal também precisa ver que alguém está transmitindo. */
    setSharing(channelId: string, peerId: string, sharing: boolean, screenProducerId: string | null = null): void {
        const member = this.channels.get(channelId)?.get(peerId);

        if (! member || (member.sharing === sharing && member.screenProducerId === screenProducerId)) {
            return;
        }

        member.sharing = sharing;
        // O id do producer vai junto para quem fechou a transmissão poder reabrir.
        member.screenProducerId = sharing ? screenProducerId : null;
        this.publish(channelId);
    }

    leave(channelId: string, peerId: string): void {
        const members = this.channels.get(channelId);

        if (!members) {
            return;
        }

        members.delete(peerId);

        if (members.size === 0) {
            this.channels.delete(channelId);
        }

        this.publish(channelId);
    }

    snapshot(serverId: string): Record<string, ChannelPresence> {
        const result: Record<string, ChannelPresence> = {};

        for (const [channelId, members] of this.channels) {
            if (this.channelServer.get(channelId) !== serverId) {
                continue;
            }

            const lista = [...members].map(([peerId, member]) => ({ peerId, ...member }));

            result[channelId] = {
                members: lista,
                // Quem não está no canal também precisa ver há quanto tempo a
                // conversa rola: o relógio local de quem entrou não serve para isso.
                startedAt: Math.min(...lista.map(member => member.joinedAt)),
            };
        }

        return result;
    }

    private publish(channelId: string): void {
        const serverId = this.channelServer.get(channelId);

        if (!serverId) {
            return;
        }

        const payload = JSON.stringify({ event: 'presence', data: { channels: this.snapshot(serverId) } });

        for (const watcher of this.watchers.get(serverId) ?? []) {
            if (watcher.socket.readyState === watcher.socket.OPEN) {
                watcher.socket.send(payload);
            }
        }
    }
}
