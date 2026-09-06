export const config = {
    listenHost: process.env.SFU_HOST ?? '127.0.0.1',
    listenPort: Number(process.env.SFU_PORT ?? 3000),
    path: process.env.SFU_PATH ?? '/sfu',
    announcedAddress: process.env.SFU_ANNOUNCED_ADDRESS ?? '127.0.0.1',
    tokenSecret: process.env.SFU_SECRET ?? '',

    // Uma porta só para toda a mídia (WebRtcServer multiplexa os transports).
    // O mediasoup é ICE Lite: nunca inicia conexão, só responde. Atrás de firewall
    // stateful isso significa que a porta PRECISA estar liberada para entrada.
    mediaPort: Number(process.env.SFU_MEDIA_PORT ?? 40000),

    worker: {
        logLevel: 'warn',
        logTags: ['info', 'ice', 'dtls', 'rtp', 'srtp', 'rtcp', 'bwe', 'score', 'simulcast', 'svc'],
    },

    router: {
        mediaCodecs: [
            { kind: 'audio', mimeType: 'audio/opus', clockRate: 48000, channels: 2, parameters: { useinbandfec: 1, usedtx: 1 } },
            { kind: 'video', mimeType: 'video/VP8', clockRate: 90000, parameters: { 'x-google-start-bitrate': 1000 } },
            { kind: 'video', mimeType: 'video/VP9', clockRate: 90000, parameters: { 'profile-id': 2, 'x-google-start-bitrate': 1000 } },
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
        ],
    },

    transport: {
        enableUdp: true,
        enableTcp: true,
        preferUdp: true,
        initialAvailableOutgoingBitrate: 10_000_000,
        maxIncomingBitrate: 12_000_000,
    },
};
