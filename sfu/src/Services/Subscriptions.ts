import type { WebSocket } from 'ws';

export type Subscriber = {
    socket: WebSocket;
    userId: string;
    name: string;
};

export type PresenceMember = { id: string; name: string };

export class Subscriptions {
    private readonly byChannel = new Map<string, Set<Subscriber>>();

    private readonly bySocket = new Map<WebSocket, Set<string>>();

    public add(channel: string, subscriber: Subscriber): boolean {
        const already = this.bySocket.get(subscriber.socket)?.has(channel) ?? false;

        if (already) {
            return false;
        }

        let members = this.byChannel.get(channel);

        if (!members) {
            members = new Set();
            this.byChannel.set(channel, members);
        }

        members.add(subscriber);

        let channels = this.bySocket.get(subscriber.socket);

        if (!channels) {
            channels = new Set();
            this.bySocket.set(subscriber.socket, channels);
        }

        channels.add(channel);

        return true;
    }

    public remove(channel: string, socket: WebSocket): Subscriber | null {
        const members = this.byChannel.get(channel);
        const gone = [...(members ?? [])].find((member) => member.socket === socket) ?? null;

        if (members && gone) {
            members.delete(gone);

            if (members.size === 0) {
                this.byChannel.delete(channel);
            }
        }

        const channels = this.bySocket.get(socket);

        channels?.delete(channel);

        if (channels?.size === 0) {
            this.bySocket.delete(socket);
        }

        return gone;
    }

    /** Tudo o que o socket ouvia, para soltar de uma vez quando ele cai. */
    public removeSocket(socket: WebSocket): string[] {
        const channels = [...(this.bySocket.get(socket) ?? [])];

        for (const channel of channels) {
            this.remove(channel, socket);
        }

        return channels;
    }

    public members(channel: string): Subscriber[] {
        return [...(this.byChannel.get(channel) ?? [])];
    }

    /**
     * Uma pessoa pode estar com o app aberto em duas máquinas e ouvir o mesmo canal duas
     * vezes. Para a lista de presença ela é uma só.
     */
    public presence(channel: string): PresenceMember[] {
        const unique = new Map<string, PresenceMember>();

        for (const member of this.members(channel)) {
            unique.set(member.userId, { id: member.userId, name: member.name });
        }

        return [...unique.values()];
    }

    public countFor(channel: string, userId: string): number {
        return this.members(channel).filter((member) => member.userId === userId).length;
    }

    public has(channel: string, socket: WebSocket): boolean {
        return this.bySocket.get(socket)?.has(channel) ?? false;
    }

    public stats(): { channels: number; subscribers: number } {
        let subscribers = 0;

        for (const members of this.byChannel.values()) {
            subscribers += members.size;
        }

        return { channels: this.byChannel.size, subscribers };
    }
}
