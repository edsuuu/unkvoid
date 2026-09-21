import { readdir, readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

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

    it('a sala anônima continua mandando o objeto puro', async () => {
        client.identity = { room: 'sala', name: 'Edsu', installId: 'i' };
        await client.setup();

        expect(joins.length).toBe(3);
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

describe('cliente do SFU, ao retomar a sessão', () => {
    it('o que mudou durante a queda chega como os eventos que se perderam', async () => {
        const client = new SfuClient();
        const events: string[] = [];
        const closed: string[] = [];

        for (const name of ['peerJoined', 'peerLeft', 'newProducer', 'producerClosed', 'producerPaused', 'peerConnectionLost']) {
            client.addEventListener(name, event => {
                const { producerId, peerId } = (event as CustomEvent).detail;

                events.push(`${name}:${producerId ?? peerId}`);
            });
        }

        client.handleMessage({ event: 'peerJoined', data: { peerId: 'ana', userId: 'user:1', name: 'Ana' } });
        client.handleMessage({ event: 'newProducer', data: { peerId: 'ana', producerId: 'tela-ana', kind: 'video', source: 'screen' } });
        client.handleMessage({ event: 'newProducer', data: { peerId: 'ana', producerId: 'mic-ana', kind: 'audio', source: 'mic' } });
        client.handleMessage({ event: 'peerJoined', data: { peerId: 'bia', userId: 'user:2', name: 'Bia' } });
        client.consumers.set('c1', { producerId: 'tela-ana', close: () => closed.push('c1') });
        client.consumerPeers.set('c1', 'ana');
        client.identity = { token: 't' };
        client.request = async () => ({
            peerId: 'eu',
            resumeKey: 'k',
            resumed: true,
            peers: [
                { peerId: 'ana', userId: 'user:1', name: 'Ana', producers: [{ producerId: 'mic-ana', kind: 'audio', source: 'mic', paused: true }] },
                { peerId: 'caio', userId: 'user:3', name: 'Caio', reconnecting: true, producers: [{ producerId: 'tela-caio', kind: 'video', source: 'screen' }] },
            ],
        });
        events.length = 0;

        await client.setup();

        expect(events).toEqual([
            'peerLeft:bia',
            'producerClosed:tela-ana',
            'producerPaused:mic-ana',
            'peerJoined:caio',
            'newProducer:tela-caio',
            'peerConnectionLost:caio',
        ]);
        expect(closed, 'o consumer da tela que acabou fecha junto').toEqual(['c1']);
        expect(client.consumers.size).toBe(0);
        expect([...client.peers.keys()]).toEqual(['ana', 'caio']);
        expect(client.peers.get('ana')?.sharing).toBe(false);
        expect(client.peers.get('caio')?.reconnecting).toBe(true);
    });

    it('a reconexão espera um tempo sorteado, para a sala inteira não voltar no mesmo instante', () => {
        const client = new SfuClient();
        const delays: number[] = [];
        const random = vi.spyOn(Math, 'random').mockReturnValue(0);
        const timer = vi.spyOn(globalThis, 'setTimeout').mockImplementation(((_handler: () => void, delay: number) => {
            delays.push(delay);

            return 0;
        }) as unknown as typeof setTimeout);

        client.scheduleReconnect();
        client.reconnectTimer = null;
        random.mockReturnValue(1);
        client.scheduleReconnect();

        random.mockRestore();
        timer.mockRestore();

        expect(delays).toEqual([500, 2000]);
    });
});

describe('cliente do SFU, sinalização viva: o navegador não avisa socket morto, então o app pergunta', () => {
    type Sent = { id: number; action: string; data: { resumeKey?: string | null; resume?: boolean } };

    class FakeSocket {
        static opened: FakeSocket[] = [];
        static pingMode: 'answer' | 'silent' | 'refuse' = 'answer';
        static unreachable = false;

        sent: Sent[] = [];
        closed = false;
        onopen: (() => void) | null = null;
        onmessage: ((message: { data: string }) => void) | null = null;
        onclose: (() => void) | null = null;
        onerror: (() => void) | null = null;

        constructor() {
            FakeSocket.opened.push(this);

            if (! FakeSocket.unreachable) {
                void Promise.resolve().then(() => this.onopen?.());
            }
        }

        send(text: string): void {
            const message = JSON.parse(text) as Sent;

            this.sent.push(message);

            if (message.action === 'join') {
                this.reply({ id: message.id, ok: true, data: { peerId: 'me', name: 'Edsu', resumeKey: 'key-1', resumed: Boolean(message.data.resume), peers: [], can: [] } });
            }

            if (message.action === 'ping' && FakeSocket.pingMode !== 'silent') {
                this.reply(FakeSocket.pingMode === 'answer' ? { id: message.id, ok: true, data: {} } : { id: message.id, ok: false, error: 'unknown action' });
            }
        }

        reply(message: object): void {
            this.onmessage?.({ data: JSON.stringify(message) });
        }

        close(): void {
            this.closed = true;
        }

        pings(): number {
            return this.sent.filter(message => message.action === 'ping').length;
        }
    }

    const enter = async () => {
        const client = new SfuClient();
        const events: string[] = [];

        client.on('diagnostic', detail => events.push(detail.event));
        client.on('reconnecting', () => events.push('reconnecting'));
        client.on('reconnected', detail => events.push(`reconnected:${detail.resumed}`));
        await client.connect('ws://sfu', { room: 'sala', name: 'Edsu', installId: 'i' });

        return { client, events, socket: FakeSocket.opened.at(-1) as FakeSocket };
    };

    beforeEach(() => {
        vi.useFakeTimers();
        vi.stubGlobal('WebSocket', FakeSocket);
        vi.spyOn(Math, 'random').mockReturnValue(0);
        FakeSocket.opened.length = 0;
        FakeSocket.pingMode = 'answer';
        FakeSocket.unreachable = false;
    });

    afterEach(() => {
        vi.useRealTimers();
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
    });

    it('depois de entrar sai um ping a cada 5 s, e o lastRttMs anda junto', async () => {
        const { client, events, socket } = await enter();

        client.lastRttMs = null;
        await vi.advanceTimersByTimeAsync(4_999);
        expect(socket.pings(), 'nada antes dos 5 s').toBe(0);

        await vi.advanceTimersByTimeAsync(10_001);

        expect(socket.pings()).toBe(3);
        expect(client.lastRttMs).not.toBeNull();
        expect(events).not.toContain('reconnecting');
        client.disconnect();
    });

    it('ping sem resposta em 10 s: larga o socket e retoma a sessão com a resumeKey', async () => {
        const { client, events, socket } = await enter();

        client.recvTransport = { close() {} };
        FakeSocket.pingMode = 'silent';
        await vi.advanceTimersByTimeAsync(14_999);
        expect(socket.closed, 'dentro do prazo o socket fica').toBe(false);

        await vi.advanceTimersByTimeAsync(1);
        expect(socket.closed).toBe(true);
        expect(events.slice(-3)).toEqual(['socket.silent', 'socket.close', 'reconnecting']);

        socket.onclose?.();
        expect(events.filter(event => event === 'reconnecting').length, 'o close atrasado do socket largado não conta de novo').toBe(1);

        FakeSocket.pingMode = 'answer';
        await vi.advanceTimersByTimeAsync(500);

        const fresh = FakeSocket.opened[1];
        const join = fresh.sent.find(message => message.action === 'join');

        expect(join?.data.resumeKey).toBe('key-1');
        expect(join?.data.resume).toBe(true);
        expect(events.at(-1)).toBe('reconnected:true');

        await vi.advanceTimersByTimeAsync(5_000);
        expect(fresh.pings(), 'o relógio volta no socket novo').toBe(1);
        client.disconnect();
    });

    it('resposta de erro prova que o socket vive: SFU antigo, que não conhece ping, não derruba ninguém', async () => {
        const { client, events, socket } = await enter();

        FakeSocket.pingMode = 'refuse';
        await vi.advanceTimersByTimeAsync(30_000);

        expect(socket.pings()).toBe(6);
        expect(socket.closed).toBe(false);
        expect(events).not.toContain('reconnecting');
        expect(events.filter(event => event === 'sfu.ping.error').length, 'a mesma recusa entra no log uma vez só').toBe(1);
        client.disconnect();
    });

    it('o ping não se empilha: com um no ar, o relógio pula a vez', async () => {
        const { client, socket } = await enter();

        FakeSocket.pingMode = 'silent';
        await vi.advanceTimersByTimeAsync(14_000);

        expect(socket.pings()).toBe(1);
        client.disconnect();
    });

    it('o relógio do ping para ao sair, ao ser expulso e quando o socket fecha', async () => {
        const left = await enter();
        const kicked = await enter();
        const dropped = await enter();

        left.client.disconnect();
        kicked.socket.reply({ event: 'kicked', data: { reason: 'expulso' } });
        FakeSocket.unreachable = true;
        dropped.socket.onclose?.();
        await vi.advanceTimersByTimeAsync(20_000);

        expect(FakeSocket.opened.length, 'só quem caiu abre socket novo').toBe(4);
        expect(FakeSocket.opened.map(socket => socket.pings())).toEqual([0, 0, 0, 0]);
        dropped.client.disconnect();
    });

    it('reconexão que falha no token fecha o socket que abriu, em vez de deixar um vivo por tentativa', async () => {
        const { client, socket } = await enter();

        client.identity = () => Promise.reject(new Error('o servidor não respondeu'));
        socket.onclose?.();
        await vi.advanceTimersByTimeAsync(500);

        expect(FakeSocket.opened[1].closed).toBe(true);
        expect(FakeSocket.opened[1].sent).toEqual([]);

        await vi.advanceTimersByTimeAsync(1_000);
        expect(FakeSocket.opened.length, 'a tentativa seguinte continua marcada').toBe(3);
        client.disconnect();
    });
});

describe('WebRTC ausente: no WebKitGTK citar o que não existe derruba o app', () => {
    const UI = resolve('ui');
    const GLOBALS = /(?<!\.)\b(RTCRtpReceiver|RTCRtpSender|RTCPeerConnection)\b/g;
    const SAFE = /(typeof\s+|globalThis\.)$/;

    it('nenhum global de WebRTC aparece cru na interface, só com typeof ou globalThis.', async () => {
        const files = (await readdir(UI, { recursive: true })).filter(name => /\.tsx?$/.test(name) && ! name.startsWith('dev'));
        const bare: string[] = [];

        for (const name of files) {
            const source = await readFile(join(UI, name), 'utf8');

            for (const hit of source.matchAll(GLOBALS)) {
                if (SAFE.test(source.slice(0, hit.index))) {
                    continue;
                }

                bare.push(`${name}:${source.slice(0, hit.index).split('\n').length} ${hit[1]}`);
            }
        }

        expect(bare, `global de WebRTC citado cru (use typeof ou globalThis.): ${bare.join(', ')}`).toEqual([]);
    });

    it('a forma segura devolve nulo em vez de levantar', () => {
        expect(globalThis.RTCRtpReceiver?.getCapabilities?.('video')?.codecs ?? null).toBeNull();
    });
});
