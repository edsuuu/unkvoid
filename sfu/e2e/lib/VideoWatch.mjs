import { NAL, nalType, parseSps, readCounterSei, toAnnexB } from './h264.mjs';

/** Freio de quadro: mais que isto entre dois quadros decodificáveis é a imagem parada. */
export const FREEZE_MS = 500;

/**
 * O que quem assiste vê de uma tela ou câmera: monta os quadros a partir dos pacotes já em
 * ordem (STAP-A, FU-A e NAL simples), decide quais um decodificador conseguiria mostrar e
 * mede o resto pelo contador do SEI.
 *
 * Um quadro é decodificável quando chegou inteiro e a corrente desde o último IDR inteiro
 * não quebrou. Buraco largado pela recuperação quebra a corrente até o próximo IDR.
 */
export class VideoWatch {
    constructor({ label = '', keepStream = false } = {}) {
        this.label = label;
        this.keepStream = keepStream;
        this.kept = [];
        this.current = null;
        this.chain = false;
        // Como o `VideoUnpacker`: só quadro furado faz esperar (e pedir) quadro-chave; antes
        // do primeiro, quem pede é o `resumeConsumer` no servidor.
        this.waitingKeyframe = false;
        this.parameterSets = { sps: null, pps: null };
        this.width = null;
        this.height = null;
        this.reset();
    }

    /** Zera o que se mede, sem esquecer a corrente: a janela de medida recomeça daqui. */
    reset(now = Date.now()) {
        this.startedAt = now;
        this.firstPacketAt = null;
        this.firstDecodableAt = null;
        this.lastDecodableAt = null;
        this.decodable = 0;
        this.broken = 0;
        this.keyframes = 0;
        this.firstIndex = null;
        this.lastIndex = null;
        this.skipped = 0;
        this.outOfOrder = 0;
        this.maxFreezeMs = 0;
        this.freezes = [];
        this.latencies = [];
        this.mismatch = null;
        // A resolução decodificada continua valendo: ela só muda no próximo IDR.
        this.resolutions = new Set(this.width ? [`${this.width}x${this.height}`] : []);
        this.packets = 0;
    }

    push(packet, gap, now = Date.now()) {
        this.packets += 1;
        this.firstPacketAt ??= now;

        if (this.current && this.current.timestamp !== packet.timestamp) {
            this.finish(now, gap);
        }

        if (!this.current) {
            this.current = { timestamp: packet.timestamp, nals: [], fragment: null, broken: gap, damaged: gap, marker: false };
        } else if (gap) {
            this.current.broken = true;
            this.current.damaged = true;
        }

        this.unpack(packet.payload);

        if (packet.marker) {
            this.current.marker = true;
            this.finish(now, false);
        }
    }

    unpack(payload) {
        const frame = this.current;
        const type = payload[0] & 0x1f;

        if (type === NAL.STAP_A) {
            for (let offset = 1; offset + 2 <= payload.length; ) {
                const size = payload.readUInt16BE(offset);

                frame.nals.push(Buffer.from(payload.subarray(offset + 2, offset + 2 + size)));
                offset += 2 + size;
            }

            return;
        }

        if (type === NAL.FU_A) {
            const start = (payload[1] & 0x80) !== 0;
            const end = (payload[1] & 0x40) !== 0;

            if (start) {
                frame.fragment = [Buffer.from([(payload[0] & 0xe0) | (payload[1] & 0x1f)]), payload.subarray(2)];
            } else if (frame.fragment) {
                frame.fragment.push(payload.subarray(2));
            } else {
                frame.broken = true;
                frame.damaged = true;
            }

            if (end && frame.fragment) {
                frame.nals.push(Buffer.concat(frame.fragment));
                frame.fragment = null;
            }

            return;
        }

        frame.nals.push(Buffer.from(payload));
    }

    /** `gapAfter`: o pacote que chegou depois de um buraco abre outro quadro; este pode ter perdido a cauda. */
    finish(now, gapAfter) {
        const frame = this.current;

        this.current = null;

        if (frame.fragment || !frame.marker) {
            frame.broken = true;
            frame.damaged ||= gapAfter;
        }

        if (frame.broken) {
            this.broken += 1;
            this.chain = false;
            // Só buraco na numeração faz o app pedir quadro-chave (`VideoUnpacker`): o quadro
            // cortado sem buraco (o consumer pausado no meio dele) ele nem percebe.
            this.waitingKeyframe ||= frame.damaged;

            return;
        }

        const types = frame.nals.map(nalType);

        for (const nal of frame.nals) {
            if (nalType(nal) === NAL.SPS) {
                this.parameterSets.sps = nal;
            }

            if (nalType(nal) === NAL.PPS) {
                this.parameterSets.pps = nal;
            }
        }

        const keyframe = types.includes(NAL.IDR) && this.parameterSets.sps && this.parameterSets.pps;

        if (keyframe) {
            const sps = parseSps(this.parameterSets.sps);

            this.chain = true;
            this.waitingKeyframe = false;
            this.keyframes += 1;
            this.width = sps.width;
            this.height = sps.height;
            this.resolutions.add(`${sps.width}x${sps.height}`);
        }

        if (!this.chain || (!keyframe && !types.includes(NAL.SLICE))) {
            this.broken += 1;

            return;
        }

        this.decoded(frame, now, keyframe);
    }

    decoded(frame, now, keyframe) {
        const counter = frame.nals.filter(nal => nalType(nal) === NAL.SEI).map(readCounterSei).find(Boolean);

        if (this.lastDecodableAt !== null) {
            const freeze = now - this.lastDecodableAt;

            this.maxFreezeMs = Math.max(this.maxFreezeMs, freeze);

            if (freeze > FREEZE_MS) {
                this.freezes.push({ at: now - this.startedAt, ms: freeze });
            }
        }

        this.firstDecodableAt ??= now;
        this.lastDecodableAt = now;
        this.decodable += 1;

        if (counter) {
            this.firstIndex ??= counter.index;

            if (this.lastIndex !== null) {
                if (counter.index <= this.lastIndex) {
                    this.outOfOrder += 1;
                } else {
                    this.skipped += counter.index - this.lastIndex - 1;
                }
            }

            this.lastIndex = counter.index;
            this.latencies.push(now - counter.sentAt);

            if (counter.width !== this.width || counter.height !== this.height) {
                this.mismatch = `${counter.width}x${counter.height} sent, ${this.width}x${this.height} decoded`;
            }
        }

        if (this.keepStream) {
            this.kept.push(...(keyframe ? frame.nals : frame.nals.filter(nal => nalType(nal) !== NAL.FILLER)));
        }
    }

    /** O que se viu desde o último `reset`, num objeto que o relatório guarda. */
    summary(now = Date.now()) {
        const elapsed = (Math.min(now, this.lastDecodableAt ?? now) - (this.firstDecodableAt ?? now)) / 1000;
        const sorted = [...this.latencies].sort((left, right) => left - right);
        const silence = this.lastDecodableAt === null ? null : now - this.lastDecodableAt;

        return {
            label: this.label,
            packets: this.packets,
            decodable: this.decodable,
            broken: this.broken,
            keyframes: this.keyframes,
            skipped: this.skipped,
            outOfOrder: this.outOfOrder,
            firstPacketMs: this.firstPacketAt === null ? null : this.firstPacketAt - this.startedAt,
            firstFrameMs: this.firstDecodableAt === null ? null : this.firstDecodableAt - this.startedAt,
            maxFreezeMs: Math.max(this.maxFreezeMs, silence ?? 0),
            freezes: this.freezes.length,
            fps: elapsed > 0 ? Math.round(((this.decodable - 1) / elapsed) * 10) / 10 : 0,
            latencyP50Ms: sorted.length ? Math.round(sorted[Math.floor(sorted.length / 2)]) : null,
            latencyP99Ms: sorted.length ? Math.round(sorted[Math.floor(sorted.length * 0.99)]) : null,
            resolution: this.width ? `${this.width}x${this.height}` : null,
            resolutions: [...this.resolutions],
            mismatch: this.mismatch ?? null,
        };
    }

    annexB() {
        return toAnnexB(this.kept);
    }
}
