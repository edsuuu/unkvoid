import { once } from 'node:events';
import { mkdirSync, openAsBlob, readdirSync, readFileSync } from 'node:fs';
import { rm } from 'node:fs/promises';
import { join } from 'node:path';

import { Recorder, type Snapshot } from './Recorder.js';
import type { Room } from './Room.js';
import { Webhook } from './Webhook.js';
import {
    ApiException,
    ForbiddenException,
    NotFoundException,
    ValidationException,
} from '../Exceptions/ApiException.js';

/** Uma política de POST do S3 assinada pelo Laravel: o SFU nunca tem credencial do bucket. */
type Upload = { url: string; fields: Record<string, string>; prefix: string };

export type ClipOrder = { clipId: string; clipper: string; streamer: string; upload: Upload };

/** Cinco minutos de vídeo copiado e áudio em AAC saem em segundos; dez minutos é travado. */
const RENDER_TIMEOUT_MS = 10 * 60_000;

const UPLOAD_TIMEOUT_MS = 5 * 60_000;

/**
 * O clipe: foto do anel no instante do pedido, e depois do 202, fora do caminho de quem
 * pediu, a montagem (MP4 e HLS com o mesmo AAC, miniatura), o upload e o aviso ao Laravel.
 */
export class Clip {
    /** O Laravel repete o pedido quando não ouve resposta: o mesmo clipe não sai duas vezes. */
    private static readonly pending = new Set<string>();

    public static order(body: string): ClipOrder {
        const data = JSON.parse(body) as Record<string, unknown>;
        const upload = data.upload as Record<string, unknown> | undefined;

        // O id vira nome de pasta: nada além de letra e número.
        if (typeof data.clipId !== 'string' || !/^[0-9A-Za-z]{1,64}$/.test(data.clipId)) {
            throw new ValidationException('field clipId is required');
        }

        for (const field of ['clipper', 'streamer'] as const) {
            if (typeof data[field] !== 'string' || data[field] === '') {
                throw new ValidationException(`field ${field} is required`);
            }
        }

        if (
            typeof upload?.url !== 'string' ||
            !/^https?:\/\//.test(upload.url) ||
            typeof upload.prefix !== 'string' ||
            typeof upload.fields !== 'object' ||
            upload.fields === null ||
            !Object.values(upload.fields).every((value) => typeof value === 'string')
        ) {
            throw new ValidationException('field upload must have url, fields and prefix');
        }

        return data as unknown as ClipOrder;
    }

    public static accept(room: Room | undefined, order: ClipOrder): void {
        if (Clip.pending.has(order.clipId)) {
            return;
        }

        if (!Recorder.available) {
            throw new ApiException('ffmpeg is not available on this server', 503);
        }

        const peers = [...(room?.peers.values() ?? [])];

        if (!peers.some((peer) => peer.userId === order.clipper && !peer.isOrphaned())) {
            throw new ForbiddenException('clipper is not in this room');
        }

        const directory = join(Recorder.root, 'clips', order.clipId);
        const snapshot = peers
            .find((peer) => peer.userId === order.streamer && peer.recorder)
            ?.recorder?.snapshot(directory);

        if (!snapshot) {
            throw new NotFoundException('streamer has no recording in this room');
        }

        Clip.pending.add(order.clipId);

        void Clip.process(order, directory, snapshot).finally(() => {
            Clip.pending.delete(order.clipId);

            return rm(directory, { recursive: true, force: true });
        });
    }

    private static async process(
        order: ClipOrder,
        directory: string,
        snapshot: Snapshot,
    ): Promise<void> {
        try {
            const durationMs = await Clip.render(directory, snapshot);
            const output = join(directory, 'out');
            // A playlist por último: enquanto ela não existe no bucket, não há clipe pela metade.
            const names = readdirSync(output).sort(
                (left, right) => Number(left === 'index.m3u8') - Number(right === 'index.m3u8'),
            );
            let sizeBytes = 0;

            for (const name of names) {
                sizeBytes += await Clip.upload(order.upload, join(output, name), name);
            }

            Webhook.post('clip.ready', { clipId: order.clipId, durationMs, sizeBytes });
        } catch (failure) {
            const reason = failure instanceof Error ? failure.message : String(failure);

            console.error(`[ERROR] clip ${order.clipId} failed: ${reason}`);
            Webhook.post('clip.failed', { clipId: order.clipId, reason: reason.slice(0, 500) });
        }
    }

    /**
     * Um AAC só: o MP4 sai com o vídeo copiado e o áudio misturado, e o HLS é o MP4
     * recortado sem recomprimir nada. Os dois têm exatamente o mesmo conteúdo.
     *
     * Cada entrada chega no tempo do anel (`-copyts`), deslocada para o keyframe do começo
     * ser o zero. O `first_pts=0` põe silêncio antes de um mic que entrou depois e corta o
     * que veio antes; o `async` preenche o silêncio de quando ele ficou mudo, que no RTP é
     * pacote nenhum.
     */
    private static async render(directory: string, snapshot: Snapshot): Promise<number> {
        const input = (names: string[]): string[] => [
            '-itsoffset',
            `-${snapshot.start}`,
            '-i',
            `concat:${names.join('|')}`,
        ];
        const args = ['-nostdin', '-loglevel', 'error', '-copyts', ...input(snapshot.video)];

        for (const names of snapshot.audio) {
            args.push(...input(names));
        }

        args.push('-map', '0:v', '-c:v', 'copy');

        if (snapshot.audio.length > 0) {
            const inputs = snapshot.audio.map(
                (_, position) =>
                    `[${position + 1}:a]aresample=48000:async=1:first_pts=0[a${position}]`,
            );
            const labels = snapshot.audio.map((_, position) => `[a${position}]`).join('');

            args.push(
                '-filter_complex',
                `${inputs.join(';')};${labels}amix=inputs=${snapshot.audio.length}:normalize=0,apad[mix]`,
                '-map',
                '[mix]',
                '-c:a',
                'aac',
                '-b:a',
                '128k',
                '-shortest',
            );
        }

        args.push('-movflags', '+faststart', 'out/clip.mp4');

        mkdirSync(join(directory, 'out'));
        await Clip.ffmpeg(args, directory);
        await Clip.ffmpeg(
            [
                '-nostdin',
                '-loglevel',
                'error',
                '-i',
                'out/clip.mp4',
                '-c',
                'copy',
                '-f',
                'hls',
                '-hls_time',
                '4',
                '-hls_playlist_type',
                'vod',
                '-hls_segment_filename',
                'out/seg-%03d.ts',
                'out/index.m3u8',
            ],
            directory,
        );

        const seconds = [
            ...readFileSync(join(directory, 'out/index.m3u8'), 'utf8').matchAll(
                /#EXTINF:([\d.]+)/g,
            ),
        ].reduce((total, match) => total + Number(match[1]), 0);

        await Clip.ffmpeg(
            [
                '-nostdin',
                '-loglevel',
                'error',
                '-ss',
                (seconds / 2).toFixed(3),
                '-i',
                'out/clip.mp4',
                '-frames:v',
                '1',
                '-vf',
                'scale=640:-2',
                // O mjpeg recusa a faixa limitada que vem do H.264.
                '-pix_fmt',
                'yuvj420p',
                'out/thumb.jpg',
            ],
            directory,
        );

        return Math.round(seconds * 1000);
    }

    /** `fields` do Laravel, a `key` e o arquivo por último. Campo a mais o MinIO recusa com 403. */
    private static async upload(upload: Upload, path: string, name: string): Promise<number> {
        const file = await openAsBlob(path);
        const form = new FormData();

        for (const [field, value] of Object.entries(upload.fields)) {
            if (field !== 'key') {
                form.append(field, value);
            }
        }

        form.append('key', `${upload.prefix}${name}`);
        form.append('file', file, name);

        const response = await fetch(upload.url, {
            method: 'POST',
            body: form,
            signal: AbortSignal.timeout(UPLOAD_TIMEOUT_MS),
        });

        if (!response.ok) {
            throw new Error(
                `upload of ${name} answered ${response.status}: ${(await response.text()).slice(0, 200)}`,
            );
        }

        return file.size;
    }

    private static async ffmpeg(args: string[], cwd: string): Promise<void> {
        const process = Recorder.ffmpeg(args, cwd, RENDER_TIMEOUT_MS);
        let errors = '';

        process.stderr?.on(
            'data',
            (chunk: Buffer) => (errors = (errors + chunk.toString()).slice(-400)),
        );

        const [code, signal] = (await once(process, 'exit')) as [number | null, string | null];

        if (code !== 0) {
            throw new Error(`ffmpeg exited with ${code ?? signal}: ${errors.trim()}`);
        }
    }
}
