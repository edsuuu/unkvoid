import { buildCounterSei, buildFiller, buildSkipSlice } from './h264.mjs';
import { MediaLibrary } from './MediaLibrary.mjs';

/** O `KeyframeGate` da 0.1.7 (`gate: 'old'`): 2 s entre pedidos, dobrando até 4 s se os pedidos não param. */
const KEYFRAME_SPACING_MS = 2000;
const MOST_KEYFRAME_SPACING_MS = 4000;
const KEYFRAME_QUIET_MS = 15_000;

/**
 * O do #53 (`gate: 'native'`): com a subida folgada, rajada de 2 no freio do SFU (0,5 s) e daí 2 e 4 s
 * enquanto os pedidos continuam; com o governador abaixo do teto (`constrained`), 2 e 4 s desde o
 * começo, como a 0.1.7. 1,5 s sem pedido recomeça a sequência.
 */
const NATIVE_SPACING_MS = 500;
const NATIVE_BURST = 2;
const NATIVE_MOST_SPACING_MS = 4000;
const NATIVE_QUIET_MS = Number(process.env.NATIVE_QUIET_MS ?? 1500);

/** O recuo do `native`. `gate: 'fast'` é a primeira versão da 3ª rodada do #53, rajada de 2 e daí 1, 2 e 4 s, 3 s quietos. */
const NATIVE_BACKOFF_MS = 2000;

/** O da 2ª rodada do #53 (`gate: 'bucket'`): 0,5 s entre pedidos, balde de 4, um a cada 2 s. */
const BUCKET_SIZE = 4;
const BUCKET_REFILL_MS = 2000;

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
        this.streak = 0;
        this.tokens = BUCKET_SIZE;
        this.countedAt = null;
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

        if ((this.gate === 'native' || this.gate === 'fast') && this.askedAt !== null && now - this.askedAt >= (this.gate === 'fast' ? 3000 : NATIVE_QUIET_MS)) {
            this.streak = 0;
        }

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
            (this.keyframeTimes ??= []).push(now);
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

    /** O `spacing` do `KeyframeGate`: a rajada só com o governador no teto; senão o recuo da 0.1.7 desde o começo. */
    nativeSpacing() {
        if (!this.constrained && this.streak < NATIVE_BURST) {
            return NATIVE_SPACING_MS;
        }

        const past = Math.max(this.streak - (this.constrained ? 1 : NATIVE_BURST), 0);

        return Math.min(NATIVE_BACKOFF_MS * 2 ** Math.min(past, 2), NATIVE_MOST_SPACING_MS);
    }

    periodicDue(now) {
        return this.lastKeyframeAt !== null && now - this.lastKeyframeAt >= this.gopMs;
    }

    requestDue(now) {
        if (this.gate === 'native' || this.gate === 'fast') {
            const spacing = this.gate === 'fast'
                ? Math.min(NATIVE_SPACING_MS * 2 ** Math.min(Math.max(this.streak - (NATIVE_BURST - 1), 0), 4), NATIVE_MOST_SPACING_MS)
                : this.nativeSpacing();

            if (!this.asked || (this.lastKeyframeAt !== null && now - this.lastKeyframeAt < spacing)) {
                return false;
            }

            this.streak += 1;

            return true;
        }

        if (this.gate === 'bucket') {
            if (this.countedAt !== null) {
                this.tokens = Math.min(BUCKET_SIZE, this.tokens + (now - this.countedAt) / BUCKET_REFILL_MS);
            }

            this.countedAt = now;

            if (!this.asked || this.tokens < 1 || (this.lastKeyframeAt !== null && now - this.lastKeyframeAt < NATIVE_SPACING_MS)) {
                return false;
            }

            this.tokens -= 1;

            return true;
        }

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
