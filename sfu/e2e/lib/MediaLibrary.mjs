import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NAL, nalType, parsePps, parseSps, splitAnnexB } from './h264.mjs';

const CACHE = join(dirname(fileURLToPath(import.meta.url)), '..', '.cache');

/**
 * A mídia de verdade que o harness manda: o quadro-chave H.264 de cada resolução e os
 * pacotes Opus, os dois saídos do ffmpeg uma vez e guardados em `e2e/.cache`.
 */
export class MediaLibrary {
    static keyframes = new Map();

    static opus = null;

    /** O IDR (SPS, PPS e a fatia) de uma resolução, no perfil baseline que o app declara. */
    static keyframe(width, height) {
        const name = `${width}x${height}`;
        const cached = MediaLibrary.keyframes.get(name);

        if (cached) {
            return cached;
        }

        const file = join(CACHE, `idr-${name}.h264`);

        if (!existsSync(file)) {
            mkdirSync(CACHE, { recursive: true });
            ffmpeg([
                '-f', 'lavfi', '-i', `testsrc2=size=${name}:rate=30,noise=alls=6:allf=t`,
                '-frames:v', '1', '-c:v', 'libx264', '-profile:v', 'baseline', '-preset', 'veryfast',
                '-threads', '1', '-x264-params', 'keyint=1:ref=1:bframes=0', '-qp', '30', '-f', 'h264', file,
            ]);
        }

        const nals = splitAnnexB(readFileSync(file)).filter(nal => [NAL.SPS, NAL.PPS, NAL.IDR].includes(nalType(nal)));
        const keyframe = {
            parameterSets: nals.filter(nal => nalType(nal) !== NAL.IDR),
            slices: nals.filter(nal => nalType(nal) === NAL.IDR),
            sps: parseSps(nals.find(nal => nalType(nal) === NAL.SPS)),
            pps: parsePps(nals.find(nal => nalType(nal) === NAL.PPS)),
            bytes: nals.reduce((total, nal) => total + nal.length, 0),
        };

        MediaLibrary.keyframes.set(name, keyframe);

        return keyframe;
    }

    /** Dez segundos de Opus a 20 ms, estéreo, de ruído rosa: cada pacote é diferente do outro. */
    static opusPackets() {
        if (MediaLibrary.opus) {
            return MediaLibrary.opus;
        }

        const file = join(CACHE, 'noise.ogg');

        if (!existsSync(file)) {
            mkdirSync(CACHE, { recursive: true });
            ffmpeg([
                '-f', 'lavfi', '-i', 'anoisesrc=d=10:c=pink:r=48000:a=0.2', '-ac', '2', '-c:a', 'libopus',
                '-b:a', '64k', '-frame_duration', '20', '-application', 'voip', file,
            ]);
        }

        MediaLibrary.opus = oggPackets(readFileSync(file)).slice(2);

        return MediaLibrary.opus;
    }
}

const ffmpeg = parameters => execFileSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', ...parameters]);

/** Os pacotes de um Ogg: cada página tem a tabela de segmentos, e 255 continua o pacote. */
const oggPackets = file => {
    const packets = [];
    let pending = [];
    let offset = 0;

    while (offset + 27 <= file.length && file.toString('latin1', offset, offset + 4) === 'OggS') {
        const segments = file[offset + 26];
        const table = file.subarray(offset + 27, offset + 27 + segments);
        let cursor = offset + 27 + segments;

        for (const size of table) {
            pending.push(file.subarray(cursor, cursor + size));
            cursor += size;

            if (size < 255) {
                packets.push(Buffer.concat(pending));
                pending = [];
            }
        }

        offset = cursor;
    }

    return packets;
};
