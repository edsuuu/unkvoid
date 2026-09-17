import { beforeAll, describe, expect, it } from 'vitest';

import { SfuClient } from '../../ui/core/SfuClient.ts';

type RequestData = { consumerId?: string; producerId?: string; source?: string; token?: string };

describe('cliente do SFU, do lado de quem assiste', () => {
    const client = new SfuClient();
    const requests: string[] = [];
    let screenConsumerId = '';

    beforeAll(() => {
        client.device = { rtpCapabilities: {} };
        client.recvTransport = { id: 'recv-1', consume: async (params: object) => ({ ...params }) };
        client.request = async (action: string, data: RequestData) => {
            requests.push(`${action}:${data.consumerId ?? data.producerId ?? ''}`);

            return {
                consumerId: `consumer-de-${data.producerId}`,
                producerId: data.producerId,
                kind: data.producerId?.startsWith('mic') ? 'audio' : 'video',
                rtpParameters: {},
                peerId: 'ana',
                name: 'Ana',
                source: data.producerId?.startsWith('mic') ? 'mic' : 'screen',
            };
        };
    });

    it('o dono do consumer vem da resposta do servidor', async () => {
        const { consumer, peerId } = await client.consume('video-producer');

        screenConsumerId = consumer.id;

        expect(peerId).toBe('ana');
        expect(client.consumersOf('ana')).toEqual([consumer.id]);
        expect(client.consumersOf('bruno')).toEqual([]);
    });

    it('pausar fala com o servidor: parar só o <video> continuaria baixando e decodificando', async () => {
        requests.length = 0;
        await client.setPeerPaused('ana', true);
        expect(requests).toEqual([`pauseConsumer:${screenConsumerId}`]);

        await client.setPeerPaused('ana', false);
        expect(requests.at(-1)).toBe(`resumeConsumer:${screenConsumerId}`);
    });

    it('pausar a tela de alguém não cala o microfone dela', async () => {
        const { consumer: mic } = await client.consume('mic-producer');

        expect(client.consumersOf('ana', 'audio')).toEqual([mic.id]);
        expect(client.consumersOf('ana').length, 'sem kind, a lista continua inteira').toBe(2);

        requests.length = 0;
        await client.setPeerPaused('ana', true, 'video');
        expect(requests, 'o mic segue tocando').toEqual([`pauseConsumer:${screenConsumerId}`]);
    });

    it('a lista de producers de cada pessoa fica viva, sem duplicar o mesmo newProducer', () => {
        client.peers.clear();
        client.trackPeers('peerJoined', { peerId: 'ana', name: 'Ana' });
        client.trackPeers('newProducer', { peerId: 'ana', producerId: 'v1', kind: 'video', source: 'screen' });
        client.trackPeers('newProducer', { peerId: 'ana', producerId: 'a1', kind: 'audio', source: 'screenAudio' });

        expect(client.peers.get('ana')?.sharing).toBe(true);
        expect(client.peers.get('ana')?.producers.map(item => item.producerId)).toEqual(['v1', 'a1']);

        client.trackPeers('newProducer', { peerId: 'ana', producerId: 'v1', kind: 'video', source: 'screen' });
        expect(client.peers.get('ana')?.producers.length).toBe(2);
    });

    it('parar a transmissão apaga o vídeo e o "compartilhando", e o áudio segue', () => {
        client.trackPeers('producerClosed', { peerId: 'ana', producerId: 'v1', kind: 'video', source: 'screen' });

        expect(client.peers.get('ana')?.sharing).toBe(false);
        expect(client.peers.get('ana')?.producers.map(item => item.producerId)).toEqual(['a1']);
    });

    it('peersChanged só quando a lista muda: um consumerClosed não redesenha ninguém', () => {
        const changes: number[] = [];

        client.addEventListener('peersChanged', () => changes.push(1));
        client.trackPeers('consumerClosed', { consumerId: 'x' });
        client.trackPeers('producerPaused', { peerId: 'ana', producerId: 'a1' });

        expect(changes.length).toBe(1);
        expect(client.peers.get('ana')?.producers[0].paused).toBe(true);
    });

    it('o nome de quem saiu chega no peerLeft, antes de a lista esquecê-lo', () => {
        const leaving = new SfuClient();
        const leftEvents: { name: string }[] = [];

        leaving.addEventListener('peerLeft', event => leftEvents.push((event as CustomEvent).detail));
        leaving.handleMessage({ event: 'peerJoined', data: { peerId: 'zeca', userId: 'user:9', name: 'Zeca' } });
        leaving.handleMessage({ event: 'peerLeft', data: { peerId: 'zeca' } });

        expect(leftEvents[0].name, 'o nome de quem saiu chega no evento').toBe('Zeca');
        expect(leaving.peers.has('zeca')).toBe(false);
    });

    it('expulso não recebe "a conexão caiu" por cima do motivo', () => {
        const kickedClient = new SfuClient();
        const closedEvents: number[] = [];

        kickedClient.addEventListener('closed', () => closedEvents.push(1));
        kickedClient.handleMessage({ event: 'kicked', data: { reason: 'expulso' } });
        kickedClient.handleClose();

        expect(closedEvents.length).toBe(0);
    });
});

describe('cliente do SFU, ao entrar e publicar', () => {
    const client = new SfuClient();
    const tokens: string[] = [];
    const joins: (string | undefined)[] = [];
    const actions: string[] = [];
    const handlers = new Map<string, (...args: unknown[]) => void>();
    const transport = {
        id: 'send-1',
        closed: false,
        on: (event: string, handler: (...args: unknown[]) => void) => handlers.set(event, handler),
        close() {
            this.closed = true;
        },
        produce: ({ track, appData, ...options }: { track: unknown; appData: unknown }) => new Promise((resolve, reject) => {
            handlers.get('produce')?.(
                { kind: 'audio', rtpParameters: { codecs: [] }, appData },
                ({ id }: { id: string }) => resolve({
                    id,
                    track,
                    appData,
                    options,
                    paused: false,
                    pause() {
                        this.paused = true;
                    },
                    resume() {
                        this.paused = false;
                    },
                    close() {},
                    on() {},
                }),
                reject,
            );
        }),
    };

    it('cada join leva um token novo: o token vale 60 s e a reconexão pede outro', async () => {
        client.identity = async () => {
            tokens.push(`token-${tokens.length + 1}`);

            return { token: tokens.at(-1) };
        };
        client.request = async (action: string, data: RequestData) => {
            if (action === 'join') {
                joins.push(data.token);
            }

            return { peerId: 'me', resumeKey: 'k', resumed: true, peers: [] };
        };

        await client.setup();
        await client.setup();

        expect(joins, 'cada join leva um token novo').toEqual(['token-1', 'token-2']);
    });

    it('mic e câmera publicados juntos criam um transporte de envio só, e o producer tem o id que o servidor devolveu', async () => {
        client.device = { rtpCapabilities: {}, createSendTransport: () => transport };
        client.request = async (action: string, data: RequestData = {}) => {
            actions.push(`${action}:${data.producerId ?? data.consumerId ?? data.source ?? ''}`);

            return action === 'createTransport' ? { transportId: 'send-1' } : action === 'produce' ? { producerId: 'p1' } : {};
        };

        const [published, twin] = await Promise.all([
            client.produce({}, 'mic', { codecOptions: { opusDtx: true } }),
            client.produce({}, 'camera'),
        ]);

        expect(published.id, 'o id do producer é o que o servidor devolveu no on(produce)').toBe('p1');
        expect(published.appData.source).toBe('mic');
        expect(published.options, 'as opções chegam ao produce do mediasoup').toEqual({ codecOptions: { opusDtx: true } });
        expect(actions.filter(action => action === 'createTransport:').length, 'mic e câmera juntos criam UM transporte').toBe(1);
        expect(actions.filter(action => action.startsWith('produce:'))).toEqual(['produce:mic', 'produce:camera']);
        expect(twin.appData.source).toBe('camera');
        expect(client.sendTransport, 'o transporte de envio fica guardado para a próxima publicação').toBeTruthy();
    });

    it('pausar, retomar e fechar o producer passam pelo servidor', async () => {
        actions.length = 0;

        await client.pauseProducer('p1');
        expect(client.producers.get('p1')?.paused).toBe(true);

        await client.resumeProducer('p1');
        expect(client.producers.get('p1')?.paused).toBe(false);

        await client.closeProducer('p1');
        expect(client.producers.has('p1')).toBe(false);
        expect(actions).toEqual(['pauseProducer:p1', 'resumeProducer:p1', 'closeProducer:p1']);
    });

    it('uma sessão nova fecha o transporte de envio da antiga, que morreu no servidor', async () => {
        client.recvTransport = { close() {} };
        client.request = async (action: string) => (action === 'join' ? { peerId: 'me2', resumeKey: 'k2', resumed: false, name: 'Edsu', peers: [] } : {});

        await client.setup();

        expect(client.sendTransport, 'sem transporte de envio depois de uma sessão nova').toBeNull();
        expect(client.sendTransportPromise).toBeNull();
        expect(transport.closed, 'o transporte antigo foi fechado').toBe(true);
    });
});
