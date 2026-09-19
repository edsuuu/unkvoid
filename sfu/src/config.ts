import type { RouterRtpCodecCapability, WorkerLogTag } from 'mediasoup/types';
import { availableParallelism } from 'node:os';

/**
 * Sem o segredo o SFU não sobe. Ele assina o token de entrada e o cabeçalho das chamadas
 * do Laravel; um servidor que subisse sem ele aceitaria qualquer um, e é melhor ficar
 * fora do ar do que aberto.
 */
const secret = process.env.SFU_SECRET ?? '';

if (secret.length < 32) {
    throw new Error('SFU_SECRET is missing or shorter than 32 characters');
}

export const config = {
    secret,

    listenHost: process.env.SFU_HOST ?? '127.0.0.1',
    listenPort: Number(process.env.SFU_PORT ?? 3000),
    path: process.env.SFU_PATH ?? '/sfu',
    announcedAddress: process.env.SFU_ANNOUNCED_ADDRESS ?? '127.0.0.1',
    appVersion: process.env.SFU_APP_VERSION ?? '0.0.3',

    // Para onde vai o aviso de quem entrou e saiu de um canal. Vazio (o padrão) desliga
    // o aviso: um SFU que sobe sem configuração não pode ficar batendo em porta alheia.
    laravelUrl: process.env.SFU_LARAVEL_URL ?? '',

    // Teto de conexões novas por IP por minuto. A sala é anônima, então o que impede
    // varrer códigos é o custo de tentar — cada tentativa precisa de um socket novo.
    // A verificação sobe um punhado de clientes de uma vez e levanta este número.
    connectionsPerMinute: Number(process.env.SFU_CONNECTIONS_PER_MINUTE ?? 20),

    // De quanto em quanto tempo perguntar a cada socket se ele continua vivo. Só é
    // configurável para a conferência poder rodar em menos de um segundo.
    heartbeatMs: Number(process.env.SFU_HEARTBEAT_MS ?? 15_000),

    // Uma porta para toda a mídia (o WebRtcServer multiplexa os transports).
    // O mediasoup é ICE Lite: ele nunca inicia conexão, só responde. Atrás de um
    // firewall com estado, isso significa que a porta PRECISA aceitar tráfego de entrada.
    mediaPort: Number(process.env.SFU_MEDIA_PORT ?? 40000),

    // Um worker por núcleo. Cada um usa uma porta a partir de mediaPort.
    workerCount: Number(process.env.SFU_WORKERS ?? availableParallelism()),

    // Ingest de RTP puro (o app nativo transmitindo para muita gente). Não pode dividir
    // a porta do WebRtcServer, e o mediasoup sortearia de 10000-59999 por padrão — uma
    // faixa estreita mantém a regra de firewall em uma linha só.
    //
    // Uma porta por sentido: quem só transmite usa uma, e quem participa da voz pelo
    // Linux usa duas (envia e recebe). Isto era 1, e como a sala inteira mora num
    // worker só, o segundo a clicar em "compartilhar" recebia `no more available
    // ports`. A faixa continua contígua: é uma linha só na regra de firewall.
    plainPortBase: Number(process.env.SFU_PLAIN_PORT ?? 41000),
    plainPortsPerWorker: Number(process.env.SFU_PLAIN_PORTS ?? 8),

    worker: {
        logLevel: 'warn' as const,
        logTags: [
            'info',
            'ice',
            'dtls',
            // Sem 'rtp': o keepalive de quem assiste por RTP puro logava "no suitable
            // Producer" a cada 5 s por espectador.
            'srtp',
            'rtcp',
            'bwe',
            'score',
            'simulcast',
            'svc',
        ] as WorkerLogTag[],
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
