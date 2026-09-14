import type { Consumer, PlainTransport, Producer, Router } from 'mediasoup/types';
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import {
    existsSync,
    linkSync,
    mkdirSync,
    readFileSync,
    readdirSync,
    rmSync,
    statSync,
    writeFileSync,
} from 'node:fs';
import { rm } from 'node:fs/promises';
import { join } from 'node:path';

import { config } from '../config.js';
import type { Peer } from './Peer.js';
import { Source } from '../Enums/Source.js';

const RING_SECONDS = 300;

/** Um segmento a mais de folga na poda: o recorte começa no primeiro keyframe da janela. */
const PRUNE_SLACK_SECONDS = 30;

const PRUNE_MS = 10_000;

/**
 * ponytail: 500 faixas gravando por processo (≈160 transmissões com mic e áudio). Cada
 * faixa gasta três portas em 127.0.0.1 logo depois da faixa de RTP puro; passar disso
 * pede um SFU_RECORDING_PORTS.
 */
const SLOTS = 500;

type Segment = { index: number; start: number; end: number };

type Track = {
    directory: string;
    slot: number | null;
    transport: PlainTransport | null;
    process: ChildProcess | null;
    exited: Promise<unknown>;
    finished: boolean;
};

/** O que um clipe leva do anel: os nomes já ligados na pasta do clipe, no tempo do anel. */
export type Snapshot = { start: number; video: string[]; audio: string[][] };

/**
 * O anel dos últimos 5 minutos de quem compartilha tela num canal: a tela, o áudio da
 * tela e o mic dessa pessoa, cada um num ffmpeg próprio que só copia o que chega em
 * segmentos de 2 s. Nada é recomprimido aqui, e nenhum pacote passa pelo Node — o
 * worker manda RTP para o ffmpeg por 127.0.0.1.
 *
 * Um processo por faixa, e não um com as três, porque mic e áudio da tela aparecem,
 * somem e ficam mudos à vontade: o vídeo não pode parar esperando por eles. O que as
 * mantém em sincronia é o relógio de parede, carimbado na chegada de cada pacote, e a
 * mistura só acontece no clique.
 */
export class Recorder {
    public static available = false;

    public static readonly root = join(config.recordingsDir, `unkvoid-sfu-${config.listenPort}`);

    private static readonly usedSlots = new Set<number>();

    /** Dez segundos antes de agora: timestamp perto de zero no MPEG-TS vira aviso de PCR. */
    private readonly origin = Date.now() / 1000 - 10;

    private readonly tracks: Track[] = [];

    private readonly directory: string;

    private readonly pruner: NodeJS.Timeout;

    private trackCount = 0;

    private stopped = false;

    private constructor(
        private readonly router: Router,
        private readonly peer: Peer,
    ) {
        this.directory = join(Recorder.root, 'rings', peer.id);
        mkdirSync(this.directory, { recursive: true });
        this.pruner = setInterval(() => this.prune(), PRUNE_MS).unref();
    }

    /**
     * O anel de uma vida anterior do processo não tem dono: quem transmitia já caiu junto.
     * E o ffmpeg precisa existir de verdade, senão o SFU sobe sem clipes em vez de cair.
     */
    public static boot(): void {
        rmSync(Recorder.root, { recursive: true, force: true });

        const probe = spawnSync('setpriv', ['--pdeathsig', 'TERM', config.ffmpeg, '-version'], {
            stdio: 'ignore',
        });

        Recorder.available = probe.status === 0;

        if (!Recorder.available) {
            console.warn(`[WARN] ${config.ffmpeg} is not executable — clips are disabled`);
        }
    }

    /**
     * O `setpriv --pdeathsig` amarra o ffmpeg à vida do SFU: um SIGKILL no Node deixaria
     * o ffmpeg de pé, segurando a porta e escrevendo num anel que ninguém mais poda.
     */
    public static ffmpeg(args: string[], cwd: string, timeout = 0): ChildProcess {
        return spawn('setpriv', ['--pdeathsig', 'KILL', config.ffmpeg, ...args], {
            cwd,
            stdio: ['ignore', 'ignore', 'pipe'],
            timeout,
        });
    }

    /** Chamado a cada producer novo. Só a conta logada que compartilha tela é gravada. */
    public static follow(router: Router, peer: Peer, producer: Producer): void {
        if (!Recorder.available || !peer.userId.startsWith('user:')) {
            return;
        }

        const source = String(producer.appData.source);

        if (source === Source.Screen && !peer.recorder) {
            // O vídeo vai copiado para MPEG-TS, e VP8 não cabe lá. A tela do app é H.264.
            if (producer.rtpParameters.codecs[0]?.mimeType.toLowerCase() !== 'video/h264') {
                return;
            }

            const recorder = new Recorder(router, peer);

            peer.recorder = recorder;
            void recorder.add(producer);

            for (const other of peer.producers.values()) {
                if (other !== producer && isRecordedAudio(other)) {
                    void recorder.add(other);
                }
            }

            return;
        }

        if (isRecordedAudio(producer)) {
            void peer.recorder?.add(producer);
        }
    }

    /**
     * Liga no pasta do clipe o que o anel tem agora. Ligação e não cópia: o anel só cria e
     * apaga arquivos, nunca reescreve, então o clipe fica com os bytes mesmo que a pessoa
     * pare de transmitir no milissegundo seguinte.
     */
    public snapshot(into: string): Snapshot | null {
        const [video, ...audios] = this.tracks;

        if (!video) {
            return null;
        }

        // O primeiro segmento sai anotado com início 0, e o recorte precisa do início real.
        const videoSegments = segments(video).filter((segment) => segment.index > 0);
        const first =
            videoSegments.find((segment) => segment.start >= this.now() - RING_SECONDS) ??
            videoSegments.at(-1);

        if (!first) {
            return null;
        }

        mkdirSync(into, { recursive: true });

        const link = (track: Track, fromIndex: number, prefix: string): string[] =>
            files(track)
                .filter((file) => file.index >= fromIndex && statSync(file.path).size > 0)
                .map((file) => {
                    const name = `${prefix}-${file.name}`;

                    linkSync(file.path, join(into, name));

                    return name;
                });

        const audio = audios
            .map((track, position) => {
                const listed = segments(track);
                const fromIndex =
                    listed.find((segment) => segment.end >= first.start)?.index ??
                    (listed.at(-1)?.index ?? -1) + 1;

                return link(track, fromIndex, `a${position}`);
            })
            .filter((names) => names.length > 0);

        return { start: first.start, video: link(video, first.index, 'v'), audio };
    }

    public stop(): void {
        if (this.stopped) {
            return;
        }

        this.stopped = true;
        clearInterval(this.pruner);

        if (this.peer.recorder === this) {
            this.peer.recorder = null;
        }

        for (const track of this.tracks) {
            this.finish(track, false);
        }

        // Só depois de todo ffmpeg morrer: um que ainda fechasse segmento recriaria o arquivo.
        void Promise.all(this.tracks.map((track) => track.exited)).then(() =>
            rm(this.directory, { recursive: true, force: true }),
        );
    }

    private async add(producer: Producer): Promise<void> {
        const isVideo = producer.kind === 'video';
        const track: Track = {
            directory: join(this.directory, String(this.trackCount)),
            slot: null,
            transport: null,
            process: null,
            exited: Promise.resolve(),
            finished: false,
        };

        this.trackCount += 1;
        this.tracks.push(track);
        mkdirSync(track.directory);

        try {
            const port = await this.openTransport(track);
            const consumer = await track.transport!.consume({
                producerId: producer.id,
                rtpCapabilities: this.router.rtpCapabilities,
                paused: true,
            });

            consumer.on('producerclose', () => (isVideo ? this.stop() : this.finish(track)));

            if (this.stopped || track.finished) {
                consumer.close();
                this.finish(track);

                return;
            }

            await track.transport!.connect({ ip: '127.0.0.1', port: port + 1 });
            this.spawn(track, consumer, port + 1);

            // O ffmpeg leva um instante para abrir a porta, e o que chega antes disso se
            // perde. O keyframe pedido depois é o que faz o primeiro segmento começar num.
            setTimeout(() => void this.start(consumer, isVideo), 1000).unref();
        } catch (failure) {
            console.error(
                `[ERROR] recorder could not follow ${String(producer.appData.source)}: ${String(failure)}`,
            );

            if (isVideo) {
                this.stop();
            } else {
                this.finish(track);
            }
        }
    }

    private async start(consumer: Consumer, isVideo: boolean): Promise<void> {
        if (consumer.closed) {
            return;
        }

        await consumer.resume();

        if (isVideo) {
            await consumer.requestKeyFrame();
        }
    }

    /** Uma porta do worker (envia) e duas do ffmpeg (RTP e o RTCP que ele insiste em abrir). */
    private async openTransport(track: Track): Promise<number> {
        const base = config.plainPortBase + config.workerCount * config.plainPortsPerWorker;
        let lastFailure: unknown = new Error('no free recording port');

        for (let slot = 0; slot < SLOTS; slot += 1) {
            if (Recorder.usedSlots.has(slot)) {
                continue;
            }

            // Marcado mesmo se o bind falhar: porta tomada por outro processo fica fora.
            Recorder.usedSlots.add(slot);

            const port = base + slot * 3;

            try {
                track.transport = await this.router.createPlainTransport({
                    listenInfo: { protocol: 'udp', ip: '127.0.0.1', port },
                    rtcpMux: true,
                    comedia: false,
                });
                track.slot = slot;

                return port;
            } catch (failure) {
                lastFailure = failure;
            }
        }

        throw lastFailure;
    }

    private spawn(track: Track, consumer: Consumer, port: number): void {
        const codec = consumer.rtpParameters.codecs[0]!;
        const [kind, name] = codec.mimeType.split('/');
        const parameters = Object.entries(codec.parameters ?? {})
            .map(([key, value]) => `${key}=${String(value)}`)
            .join(';');

        writeFileSync(
            join(track.directory, 'track.sdp'),
            [
                'v=0',
                'o=- 0 0 IN IP4 127.0.0.1',
                's=unkvoid',
                'c=IN IP4 127.0.0.1',
                't=0 0',
                `m=${kind} ${port} RTP/AVP ${codec.payloadType}`,
                `a=rtpmap:${codec.payloadType} ${name}/${codec.clockRate}${codec.channels ? `/${codec.channels}` : ''}`,
                ...(parameters ? [`a=fmtp:${codec.payloadType} ${parameters}`] : []),
                '',
            ].join('\n'),
        );

        const process = Recorder.ffmpeg(
            [
                '-nostdin',
                '-loglevel',
                'error',
                '-protocol_whitelist',
                'file,udp,rtp',
                '-localaddr',
                '127.0.0.1',
                // Mic mudo não manda pacote nenhum: sem isto o ffmpeg desiste em 10 s.
                '-listen_timeout',
                '-1',
                '-use_wallclock_as_timestamps',
                '1',
                // Na entrada e não na saída: o `-output_ts_offset` não chega no vídeo copiado.
                '-itsoffset',
                `-${this.origin.toFixed(3)}`,
                '-i',
                'track.sdp',
                '-map',
                '0',
                '-c',
                'copy',
                '-copyts',
                '-f',
                'segment',
                '-segment_time',
                '2',
                // Corta pelo relógio, não pela contagem: depois de um mic mudo por minutos,
                // a contagem atrasada cortaria um segmento a cada pacote até alcançar.
                '-segment_atclocktime',
                '1',
                '-segment_list',
                'segments.csv',
                '-segment_list_type',
                'csv',
                '-segment_list_size',
                '400',
                '-segment_format',
                'mpegts',
                // Sem flush o áudio do segmento aberto fica segundos no buffer do ffmpeg, e
                // o clipe perderia o fim da fala.
                '-segment_format_options',
                'mpegts_copyts=1:flush_packets=1',
                'seg-%06d.ts',
            ],
            track.directory,
        );

        track.process = process;
        track.exited = new Promise((resolve) => process.once('exit', resolve));
        // Sem ouvinte, um `error` do processo filho derrubaria o SFU inteiro.
        process.on('error', (failure) =>
            console.error(`[ERROR] recorder ffmpeg: ${failure.message}`),
        );
        process.stderr?.on('data', (chunk: Buffer) =>
            console.warn(`[WARN] recorder ffmpeg: ${chunk.toString().trim()}`),
        );
        process.on('exit', (code, signal) => {
            if (!track.finished) {
                console.warn(`[WARN] recorder ffmpeg exited on its own (${code ?? signal})`);
            }
        });
    }

    /** Fecha uma faixa. O mic que para fica no anel até sair da janela de 5 minutos. */
    private finish(track: Track, graceful = true): void {
        if (track.finished) {
            return;
        }

        track.finished = true;

        if (graceful) {
            // Parado num UDP sem pacote, o ffmpeg só larga a leitura no segundo sinal. Dois
            // sinais diferentes porque dois iguais seguidos o kernel entrega como um.
            track.process?.kill('SIGTERM');
            track.process?.kill('SIGINT');
        } else {
            track.process?.kill('SIGKILL');
        }

        track.transport?.close();

        if (track.slot !== null) {
            Recorder.usedSlots.delete(track.slot);
        }
    }

    private prune(): void {
        const horizon = this.now() - RING_SECONDS - PRUNE_SLACK_SECONDS;

        for (const track of [...this.tracks]) {
            const listed = segments(track);
            // O segmento aberto ainda não está na lista. Faixa encerrada não tem aberto.
            const keepFrom =
                listed.find((segment) => segment.end >= horizon)?.index ??
                (track.finished ? Number.POSITIVE_INFINITY : (listed.at(-1)?.index ?? -1) + 1);
            const remaining = files(track).filter((file) => {
                if (file.index >= keepFrom) {
                    return true;
                }

                rmSync(file.path, { force: true });

                return false;
            });

            if (track.finished && remaining.length === 0 && this.tracks[0] !== track) {
                this.tracks.splice(this.tracks.indexOf(track), 1);
                rmSync(track.directory, { recursive: true, force: true });
            }
        }
    }

    /** Agora, no relógio em que os segmentos foram carimbados. */
    private now(): number {
        return Date.now() / 1000 - this.origin;
    }
}

const isRecordedAudio = (producer: Producer): boolean =>
    producer.appData.source === Source.Mic || producer.appData.source === Source.ScreenAudio;

/** Os segmentos fechados, na ordem, como o ffmpeg os anotou. */
const segments = (track: Track): Segment[] => {
    const list = join(track.directory, 'segments.csv');

    if (!existsSync(list)) {
        return [];
    }

    return readFileSync(list, 'utf8')
        .split('\n')
        .map((line) => line.split(','))
        .filter((fields) => fields.length === 3 && /^seg-\d+\.ts$/.test(fields[0]!))
        .map(([name, start, end]) => ({
            index: Number(name!.slice(4, -3)),
            start: Number(start),
            end: Number(end),
        }))
        .sort((left, right) => left.index - right.index);
};

const files = (track: Track): { name: string; path: string; index: number }[] =>
    readdirSync(track.directory)
        .filter((name) => /^seg-\d+\.ts$/.test(name))
        .map((name) => ({
            name,
            path: join(track.directory, name),
            index: Number(name.slice(4, -3)),
        }));
