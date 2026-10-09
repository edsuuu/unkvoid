import { buildCounterSei, buildFiller, buildSkipSlice } from './h264.mjs';
import { MediaLibrary } from './MediaLibrary.mjs';

/** O espaço mínimo entre dois quadros-chave pedidos, como o `KeyframeGate` do app. */
const KEYFRAME_SPACING_MS = 2000;
const MOST_KEYFRAME_SPACING_MS = 4000;
const KEYFRAME_QUIET_MS = 15_000;

/** O GOP do encoder do Windows: um quadro-chave periódico a cada 4 s. */
const GOP_MS = 4000;

/**
 * O encoder de uma origem de vídeo (tela ou câmera), sem encoder: o IDR pronto da
 * resolução pedida, quadros P `P_Skip` com enchimento até a taxa, e o contador em cada um.
 *
 * `gate: 'native'` atende o pedido de quadro-chave como o app (espaço de 2 s, dobrando até
 * 4 s se os pedidos não param); `gate: 'immediate'` atende no quadro seguinte, para medir
 * o SFU sem o freio do app.
 */
export class VideoTrack {
    constructor({ width, height, fps, bitrate, gate = 'native', gopMs = GOP_MS }) {
        this.fps = fps;
        this.bitrate = bitrate;
        this.gate = gate;
        this.gopMs = gopMs;
        this.index = 0;
        this.sinceKeyframe = 0;
        this.asked = false;
        this.askedAt = null;
        this.waited = false;
        this.spacing = KEYFRAME_SPACING_MS;
        this.lastKeyframeAt = null;
        this.keyframesSent = 0;
        this.keyframeRequests = 0;
        this.resize(width, height);
    }

    /** Trocar a resolução é o encoder recomeçar: o quadro seguinte é um IDR do tamanho novo. */
    resize(width, height) {
        this.width = width;
        this.height = height;
        this.keyframe = MediaLibrary.keyframe(width, height);
        this.forceKeyframe = true;
    }

    requestKeyframe(now = Date.now()) {
        this.keyframeRequests += 1;
        this.asked = true;
        this.askedAt = now;
    }

    next(now = Date.now()) {
        const keyframe = this.forceKeyframe || this.periodicDue(now) || this.requestDue(now);
        const index = this.index;
        const sei = buildCounterSei({ index, sentAt: now, width: this.width, height: this.height });
        let nals;

        if (keyframe) {
            this.forceKeyframe = false;
            this.asked = false;
            this.waited = false;
            this.lastKeyframeAt = now;
            this.sinceKeyframe = 0;
            this.keyframesSent += 1;
            // O SEI vai depois do SPS: o consumer recém-retomado do mediasoup só começa a
            // repassar no pacote que traz o SPS, e o que vem antes dele no quadro se perde.
            nals = [...this.keyframe.parameterSets, sei, ...this.keyframe.slices];
        } else {
            this.sinceKeyframe += 1;

            const slice = buildSkipSlice(this.keyframe.sps, this.keyframe.pps, this.sinceKeyframe);
            const target = Math.round(this.bitrate / this.fps / 8);
            const filler = target - sei.length - slice.length;

            nals = filler > 64 ? [sei, slice, buildFiller(filler)] : [sei, slice];
        }

        this.index += 1;

        return { index, keyframe, nals, capturedAt: now };
    }

    periodicDue(now) {
        return this.lastKeyframeAt !== null && now - this.lastKeyframeAt >= this.gopMs;
    }

    requestDue(now) {
        if (!this.asked) {
            if (this.askedAt !== null && now - this.askedAt >= KEYFRAME_QUIET_MS) {
                this.spacing = KEYFRAME_SPACING_MS;
            }

            return false;
        }

        if (this.gate === 'immediate') {
            return true;
        }

        if (this.lastKeyframeAt !== null && now - this.lastKeyframeAt < this.spacing) {
            this.waited = true;

            return false;
        }

        if (this.waited) {
            this.spacing = Math.min(this.spacing * 2, MOST_KEYFRAME_SPACING_MS);
        }

        return true;
    }
}
