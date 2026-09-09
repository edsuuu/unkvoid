import { availableParallelism } from 'node:os';

import type { RouterRtpCodecCapability, WorkerLogTag } from 'mediasoup/types';

export const config = {
    listenHost: process.env.SFU_HOST ?? '127.0.0.1',
    listenPort: Number(process.env.SFU_PORT ?? 3000),
    path: process.env.SFU_PATH ?? '/sfu',
    announcedAddress: process.env.SFU_ANNOUNCED_ADDRESS ?? '127.0.0.1',

    // Teto de conexões novas por IP por minuto. A sala é anônima, então o que impede
    // varrer códigos é o custo de tentar — cada tentativa precisa de um socket novo.
    // A verificação sobe um punhado de clientes de uma vez e levanta este número.
    connectionsPerMinute: Number(process.env.SFU_CONNECTIONS_PER_MINUTE ?? 20),

    // One port for all media (WebRtcServer multiplexes transports).
    // mediasoup is ICE Lite: it never initiates a connection, only responds. Behind a firewall
    // stateful, this means the port MUST allow inbound traffic.
    mediaPort: Number(process.env.SFU_MEDIA_PORT ?? 40000),

    // One worker per core. Each one uses a port starting at mediaPort.
    workerCount: Number(process.env.SFU_WORKERS ?? availableParallelism()),

    // Ingest de RTP puro (o app nativo transmitindo para muita gente). Não pode dividir
    // a porta do WebRtcServer, e o mediasoup sortearia de 10000-59999 por padrão — uma
    // faixa estreita mantém a regra de firewall em uma linha só.
    //
    // Uma porta por worker: vídeo e áudio de uma transmissão dividem o mesmo transport,
    // então isto é uma transmissão simultânea por worker. Subir o número aqui é barato,
    // mas cada porta a mais é uma regra de firewall que alguém abre à mão.
    plainPortBase: Number(process.env.SFU_PLAIN_PORT ?? 41000),
    plainPortsPerWorker: Number(process.env.SFU_PLAIN_PORTS ?? 1),

    worker: {
        logLevel: 'warn' as const,
        logTags: ['info', 'ice', 'dtls', 'rtp', 'srtp', 'rtcp', 'bwe', 'score', 'simulcast', 'svc'] as WorkerLogTag[],
    },

    router: {
        mediaCodecs: [
            {
                kind: 'audio',
                mimeType: 'audio/opus',
                clockRate: 48000,
                channels: 2,
                parameters: { useinbandfec: 1, usedtx: 1 },
            },
            {
                kind: 'video',
                mimeType: 'video/VP8',
                clockRate: 90000,
                parameters: { 'x-google-start-bitrate': 1000 },
            },
            {
                kind: 'video',
                mimeType: 'video/VP9',
                clockRate: 90000,
                parameters: { 'profile-id': 2, 'x-google-start-bitrate': 1000 },
            },
            {
                kind: 'video',
                mimeType: 'video/H264',
                clockRate: 90000,
                parameters: {
                    'packetization-mode': 1,
                    'profile-level-id': '42e01f',
                    'level-asymmetry-allowed': 1,
                    'x-google-start-bitrate': 1000,
                },
            },
        ] as RouterRtpCodecCapability[],
    },

    transport: {
        enableUdp: true,
        enableTcp: true,
        preferUdp: true,
        initialAvailableOutgoingBitrate: 10_000_000,
        maxIncomingBitrate: 12_000_000,
    },
} as const;
