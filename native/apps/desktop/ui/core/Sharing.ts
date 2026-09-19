import type { App } from './App.ts';
import { Failure } from './Failure.ts';
import { Platform } from './Platform.ts';
import { Store } from './Store.ts';
import { Tauri } from './Tauri.ts';

export type ShareTab = 'display' | 'window';

export type SourceItem = { value: string; label: string; detail: string };

export type BroadcastStats = {
    active: boolean;
    captured: number;
    sent: number;
    sentBytes: number;
    sendDropped: number;
    encodeErrors: number;
    sendErrors: number;
    audioErrors: number;
    busyUs?: number;
    encoder?: string;
    captureError?: string;
    targetBitrate?: number;
    lossPermille?: number;
};

export type BroadcastLine =
    | { starting: true }
    | { starting?: false; fps: number; mbps: number; dropped: number; encoder: string | null; lossPercent: number | null; reducedToMbps: number | null };

export type SharingState = {
    open: boolean;
    loading: boolean;
    tab: ShareTab;
    sources: Record<ShareTab, SourceItem[]>;
    previews: Record<string, string>;
    source: string | null;
    audio: boolean;
    muteCalls: boolean;
    quality: string;
    fps: string;
    active: boolean;
    starting: boolean;
    line: BroadcastLine | null;
};

type Display = { id: number; width: number; height: number };

type AppWindow = { id: number; title: string; application: string };

export class Sharing {
    static readonly QUALITY_KEY = 'unkvoid:quality';
    static readonly FPS_KEY = 'unkvoid:fps';
    static readonly QUALITIES = ['720', '1080', '1440', '2160'];
    static readonly FRAME_RATES = ['30', '60'];
    static readonly MAX_WINDOW_SOURCES = 12;
    static readonly MAX_WINDOW_PREVIEWS = 4;

    static numbers(line: BroadcastLine | null): string {
        if (! line || line.starting) {
            return '';
        }

        const loss = line.lossPercent === null ? '' : ` · perda ${line.lossPercent.toFixed(1)}%`;
        const reduced = line.reducedToMbps === null ? '' : ` · internet apertada: reduzido para ${line.reducedToMbps.toFixed(1)} Mb/s`;

        return `${line.fps} fps · ${line.mbps.toFixed(1)} Mb/s · ${line.dropped} perdidos${loss}${reduced}`;
    }

    readonly app: App;
    readonly previewInFlight = new Set<string>();
    statsTimer: number | null = null;
    lastStats: BroadcastStats | null = null;
    statsAt = 0;
    statsGeneration = 0;
    ceilingBitrate = 0;
    cpuEncoderWarned = false;
    readonly store: Store<SharingState>;

    constructor(app: App) {
        this.app = app;
        this.store = new Store<SharingState>({
            open: false,
            loading: false,
            tab: 'display',
            sources: { display: [], window: [] },
            previews: {},
            source: null,
            audio: true,
            muteCalls: true,
            quality: '1080',
            fps: '60',
            active: false,
            starting: false,
            line: null,
        });
    }

    loadPreferences(): void {
        const cores = navigator.hardwareConcurrency ?? 4;
        const guess = cores > 8
            ? { quality: '1080', fps: '60' }
            : { quality: cores <= 4 ? '720' : '1080', fps: cores <= 4 || Platform.isLinux() ? '30' : '60' };
        const savedQuality = localStorage.getItem(Sharing.QUALITY_KEY) ?? '';
        const savedFps = localStorage.getItem(Sharing.FPS_KEY) ?? '';

        this.store.set({
            quality: Sharing.QUALITIES.includes(savedQuality) ? savedQuality : guess.quality,
            fps: Sharing.FRAME_RATES.includes(savedFps) ? savedFps : guess.fps,
        });
    }

    setQuality(quality: string): void {
        localStorage.setItem(Sharing.QUALITY_KEY, quality);
        this.store.set({ quality });
    }

    setFps(fps: string): void {
        localStorage.setItem(Sharing.FPS_KEY, fps);
        this.store.set({ fps });
    }

    async changeQuality(quality: string, fps: string): Promise<void> {
        const broadcast = this.app.media.broadcast;
        const previous = { quality: this.store.state.quality, fps: this.store.state.fps };

        if (! broadcast || ! this.store.state.active) {
            return;
        }

        this.setQuality(quality);
        this.setFps(fps);
        this.app.log('broadcast.quality', { quality, fps });

        try {
            await broadcast.changeQuality(quality, Number(fps));
            this.statsGeneration += 1;
            this.ceilingBitrate = 0;
            this.app.toast(`transmitindo em ${quality === '2160' ? '4K' : `${quality}p`} a ${fps} fps`);
        } catch (failure) {
            this.setQuality(previous.quality);
            this.setFps(previous.fps);
            this.app.log('broadcast.quality.error', { message: Failure.message(failure) });
            this.app.fail(`não deu para trocar a qualidade: ${Failure.message(failure)}`);
        }
    }

    setAudio(audio: boolean): void {
        this.store.set({ audio });
    }

    setMuteCalls(muteCalls: boolean): void {
        this.store.set({ muteCalls });
    }

    async open(): Promise<void> {
        this.store.set({ open: true, loading: true, source: null, previews: {} });

        const listOrNone = <Item>(command: string): Promise<Item[]> => Tauri.invoke<Item[]>(command).catch((failure: unknown) => {
            this.app.log(`share.${command}.error`, { message: Failure.message(failure) });

            return [];
        });
        const [displays, appWindows] = await Promise.all([listOrNone<Display>('list_displays'), listOrNone<AppWindow>('list_windows')]);

        this.store.set({
            loading: false,
            sources: {
                display: (displays ?? []).map(display => ({
                    value: `display:${display.id}`,
                    label: `Tela ${display.id}`,
                    detail: display.width ? `${display.width}×${display.height}` : '',
                })),
                window: (appWindows ?? [])
                    .filter(appWindow => appWindow.title.trim() !== '')
                    .slice(0, Sharing.MAX_WINDOW_SOURCES)
                    .map(appWindow => ({ value: `window:${appWindow.id}`, label: appWindow.title, detail: appWindow.application })),
            },
        });

        this.setTab('display');
    }

    setTab(tab: ShareTab): void {
        const items = this.store.state.sources[tab] ?? [];

        this.store.set({ tab, source: items[0]?.value ?? null });

        items.forEach((item, index) => {
            if (tab === 'window' && index >= Sharing.MAX_WINDOW_PREVIEWS) {
                return;
            }

            void this.loadPreview(item);
        });
    }

    async loadPreview(item: SourceItem): Promise<void> {
        if (this.previewInFlight.has(item.value) || this.store.state.previews[item.value]) {
            return;
        }

        this.previewInFlight.add(item.value);

        try {
            const data = await Tauri.invoke<string | null>('source_preview', { source: item.value });

            if (data && this.store.state.open) {
                this.store.set(state => ({ previews: { ...state.previews, [item.value]: data } }));
            }
        } catch (failure) {
            this.app.log('share.preview.error', { source: item.value, message: Failure.message(failure) });
        } finally {
            this.previewInFlight.delete(item.value);
        }
    }

    pick(source: string): void {
        this.store.set({ source });
    }

    close(): void {
        this.previewInFlight.clear();
        this.store.set({ open: false });
    }

    async confirm(): Promise<void> {
        this.close();

        if (this.store.state.active) {
            await this.stop();
        }

        await this.start();
    }

    async start(): Promise<void> {
        const { quality, fps, source, audio, muteCalls } = this.store.state;
        const broadcast = this.app.media.broadcast;
        const withoutCalls = audio && muteCalls;

        this.app.log('broadcast.start', { quality, fps, source, audio, muteCalls: withoutCalls });

        if (! broadcast || ! source) {
            return;
        }

        this.store.set({ starting: true });

        try {
            await broadcast.start(quality, Number(fps), source, audio, withoutCalls);
            this.statsTimer = setInterval(() => void this.readStats(), 1000);
            void this.readStats();
            this.paint(true);
        } catch (failure) {
            this.app.log('broadcast.start.error', { message: Failure.message(failure) });
            this.paint(false);
            this.app.fail(`não deu para transmitir: ${Failure.message(failure)}`);
        } finally {
            this.store.set({ starting: false });
        }
    }

    readStats(): Promise<void> {
        const generation = this.statsGeneration;

        return Tauri.invoke<BroadcastStats | null>('broadcast_stats')
            .then(stats => {
                if (generation === this.statsGeneration) {
                    this.updateStats(stats);
                }
            })
            .catch((failure: unknown) => this.app.log('broadcast.stats.error', { message: Failure.message(failure) }));
    }

    paint(on: boolean): void {
        if (on !== this.store.state.active) {
            this.app.sounds[on ? 'streamStarted' : 'streamStopped']();
        }

        this.store.set({ active: on });

        if (on) {
            return;
        }

        const media = this.app.media;

        if (media.sfu?.peerId) {
            media.showScreen(media.sfu.peerId, null);
        }

        media.store.set({ selfView: false });
        clearInterval(this.statsTimer ?? undefined);
        this.statsTimer = null;
        this.lastStats = null;
        this.statsAt = 0;
        this.statsGeneration += 1;
        this.ceilingBitrate = 0;
        this.store.set({ line: null });
    }

    updateStats(stats: BroadcastStats | null): void {
        if (! stats?.active) {
            return;
        }

        const now = performance.now();
        const previous = this.lastStats;
        const elapsed = this.statsAt ? Math.max(now - this.statsAt, 1) : 1000;
        const seconds = elapsed / 1000;
        const target = Number.isFinite(stats.targetBitrate) && stats.targetBitrate! > 0 ? stats.targetBitrate! : null;

        this.ceilingBitrate = Math.max(this.ceilingBitrate, target ?? 0);

        this.store.set({
            line: previous
                ? {
                    fps: Math.round((stats.sent - previous.sent) / seconds),
                    mbps: (stats.sentBytes - previous.sentBytes) * 8 / seconds / 1e6,
                    dropped: stats.sendDropped,
                    encoder: stats.encoder ?? null,
                    lossPercent: Number.isFinite(stats.lossPermille) ? stats.lossPermille! / 10 : null,
                    reducedToMbps: target !== null && target < this.ceilingBitrate ? target / 1e6 : null,
                }
                : { starting: true },
        });

        this.app.log('broadcast.stats', {
            ...stats,
            pingMs: this.app.media.sfu?.transportRttMs ?? this.app.media.sfu?.lastRttMs ?? null,
            fps: previous ? Math.round((stats.captured - previous.captured) * 1000 / elapsed) : null,
        });

        if (stats.encoder === 'cpu' && ! this.cpuEncoderWarned) {
            this.cpuEncoderWarned = true;
            this.app.log('broadcast.encoder', { encoder: stats.encoder });
            this.app.toast(`sem encoder na placa de vídeo: transmitindo pelo processador${Platform.isLinux() ? '' : ', em 720p30'}`);
        }

        if (previous) {
            const errors = {
                encodeErrors: stats.encodeErrors - previous.encodeErrors,
                sendErrors: stats.sendErrors - previous.sendErrors,
                audioErrors: stats.audioErrors - previous.audioErrors,
            };

            if (Object.values(errors).some(value => value > 0)) {
                this.app.log('broadcast.error', { reason: 'media pipeline error', ...errors, totals: stats });
            }
        }

        this.lastStats = stats;
        this.statsAt = now;
    }

    async died(detail: { source?: string; reason?: string } | null): Promise<void> {
        if (detail?.source !== 'screen' && detail?.source !== 'screenAudio') {
            this.app.log('broadcast.dead.ignored', detail);

            return;
        }

        this.app.log('broadcast.dead', detail);

        if (! this.store.state.active) {
            return;
        }

        if (detail.reason === 'revoked') {
            await this.revoked();

            return;
        }

        if (detail.source === 'screenAudio') {
            this.app.toast('o áudio do sistema não chegou ao servidor: a transmissão segue sem som', true);

            return;
        }

        this.store.set({ active: false });

        const last = this.lastStats;

        await this.stop();

        if (last && last.captured === 0) {
            const reason = last.captureError ? `: ${last.captureError}` : '. Rode `unkvoid-desktop --check-capture` num terminal para ver o motivo.';

            this.app.fail(`a captura de vídeo não gerou nenhum quadro${reason}`);

            return;
        }

        this.app.fail('a transmissão não chegou ao servidor: nenhum pacote entrou em 30 s. A porta de RTP está bloqueada no caminho.');
    }

    async revoked(): Promise<void> {
        if (! this.store.state.active) {
            return;
        }

        this.app.log('broadcast.revoked');
        this.store.set({ active: false });
        await this.stop();
        this.app.fail('você perdeu a permissão de transmitir neste canal: a transmissão foi encerrada.');
    }

    async stop(): Promise<void> {
        this.app.log('broadcast.stop');
        clearInterval(this.statsTimer ?? undefined);
        this.statsTimer = null;
        await this.app.media.broadcast?.stop().catch((failure: unknown) => {
            this.app.log('broadcast.stop.error', { message: Failure.message(failure) });
            this.app.fail(`a captura pode não ter parado: ${Failure.message(failure)}. Se o jogo continuar pesado, feche e abra o app.`);
        });
        this.paint(false);
    }
}
