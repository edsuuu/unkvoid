import type { PlainProducerResponse, SfuClient } from './SfuClient.ts';
import { Tauri } from './Tauri.ts';

export class Broadcast {
    readonly sfu: SfuClient;
    broadcasting = false;
    nativeActive = false;
    producerIds: string[] = [];
    videoProducerId: string | null = null;
    withAudio = true;

    constructor(sfu: SfuClient) {
        this.sfu = sfu;
    }

    async start(quality: string, fps: number, source: string, audio: boolean, muteCalls: boolean): Promise<void> {
        await Tauri.invoke('start_broadcast', { quality, fps, source, audio, muteCalls });
        this.nativeActive = true;
        this.withAudio = audio;

        try {
            await this.publish();
        } catch (failure) {
            await this.stop();
            throw failure;
        }
    }

    async republish(): Promise<boolean> {
        if (! this.nativeActive) {
            return false;
        }

        await Tauri.invoke('renew_sfu_key');

        this.producerIds = [];
        this.videoProducerId = null;

        await this.publish();

        return true;
    }

    async publish(): Promise<void> {
        let target: PlainProducerResponse | null = null;

        const sources = this.withAudio ? (['screen', 'screenAudio'] as const) : (['screen'] as const);

        for (const source of sources) {
            const kind = source === 'screen' ? 'video' : 'audio';
            const offer = await Tauri.invoke<Record<string, unknown>>('sfu_offer', { source });

            const producer = await this.sfu.request<PlainProducerResponse>('producePlain', { kind, source, ...offer });

            this.producerIds.push(producer.producerId);

            if (source === 'screen') {
                this.videoProducerId = producer.producerId;
            }

            target = producer;
        }

        const { ip, port, srtpParameters } = target!;

        await Tauri.invoke('use_sfu', {
            address: `${ip}:${port}`,
            serverKey: srtpParameters?.keyBase64 ?? null,
        });
        this.broadcasting = true;
    }

    async stop(): Promise<number> {
        if (! this.nativeActive && ! this.producerIds.length) {
            return 0;
        }

        this.broadcasting = false;

        const producerIds = this.producerIds.splice(0);

        this.videoProducerId = null;

        await Promise.all(producerIds.map(producerId => this.sfu.tolerate('closeProducer', { producerId })));

        try {
            return await Tauri.invoke<number>('stop_broadcast');
        } finally {
            this.nativeActive = false;
        }
    }
}
