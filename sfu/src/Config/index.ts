import type { RouterRtpCodecCapability, WorkerLogTag } from 'mediasoup/types';
import { availableParallelism } from 'node:os';

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

    laravelUrl: process.env.SFU_LARAVEL_URL ?? '',

    corsOrigins: (process.env.CORS_URL ?? '*')
        .split(',')
        .map((origin) => origin.trim())
        .filter((origin) => origin !== ''),

    connectionsPerMinute: Number(process.env.SFU_CONNECTIONS_PER_MINUTE ?? 20),

    heartbeatMs: Number(process.env.SFU_HEARTBEAT_MS ?? 15_000),

    mediaPort: Number(process.env.SFU_MEDIA_PORT ?? 40000),

    workerCount: Number(process.env.SFU_WORKERS ?? availableParallelism()),

    plainPortBase: Number(process.env.SFU_PLAIN_PORT ?? 41000),
    plainPortsPerWorker: Number(process.env.SFU_PLAIN_PORTS ?? 8),

    worker: {
        logLevel: 'warn' as const,
        logTags: [
            'info',
            'ice',
            'dtls',

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
