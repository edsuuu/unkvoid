import { createHash } from 'node:crypto';

import { MediaLibrary } from './MediaLibrary.mjs';

/**
 * O som que chega de um microfone: cada pacote Opus é reconhecido pelo conteúdo (o ruído
 * rosa não se repete em dez segundos), e a posição dele no laço diz se veio em ordem e
 * quantos faltaram. O relógio RTP tem de andar 960 por pacote.
 */
export class AudioWatch {
    static positions = null;

    constructor({ label = '' } = {}) {
        this.label = label;
        AudioWatch.positions ??= new Map(MediaLibrary.opusPackets().map((packet, index) => [hash(packet), index]));
        this.total = MediaLibrary.opusPackets().length;
        this.reset();
    }

    reset(now = Date.now()) {
        this.startedAt = now;
        this.packets = 0;
        this.unknown = 0;
        this.lost = 0;
        this.outOfOrder = 0;
        this.lastPosition = null;
        this.lastTimestamp = null;
        this.clockErrors = 0;
        this.firstAt = null;
        this.lastAt = null;
        this.maxGapMs = 0;
    }

    push(packet, now = Date.now()) {
        this.packets += 1;
        this.firstAt ??= now;

        if (this.lastAt !== null) {
            this.maxGapMs = Math.max(this.maxGapMs, now - this.lastAt);
        }

        this.lastAt = now;

        const position = AudioWatch.positions.get(hash(packet.payload));

        if (position === undefined) {
            this.unknown += 1;

            return;
        }

        if (this.lastPosition !== null) {
            const step = (position - this.lastPosition + this.total) % this.total;

            if (step === 0 || step > this.total / 2) {
                this.outOfOrder += 1;
            } else {
                this.lost += step - 1;

                const ticks = (packet.timestamp - this.lastTimestamp) >>> 0;

                if (ticks !== step * 960) {
                    this.clockErrors += 1;
                }
            }
        }

        this.lastPosition = position;
        this.lastTimestamp = packet.timestamp;
    }

    summary(now = Date.now()) {
        return {
            label: this.label,
            packets: this.packets,
            lost: this.lost,
            unknown: this.unknown,
            outOfOrder: this.outOfOrder,
            clockErrors: this.clockErrors,
            firstPacketMs: this.firstAt === null ? null : this.firstAt - this.startedAt,
            maxGapMs: Math.max(this.maxGapMs, this.lastAt === null ? 0 : now - this.lastAt),
        };
    }
}

const hash = payload => createHash('sha1').update(payload).digest('base64');
