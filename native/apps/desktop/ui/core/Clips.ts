import Hls from 'hls.js/light';

import type { App } from './App.ts';
import { Failure } from './Failure.ts';
import type { Hub } from './Hub.ts';
import type { Clip } from './Models.ts';
import { Store } from './Store.ts';
import { Tauri } from './Tauri.ts';

export type ClipsState = {
    clips: Clip[];
    loading: boolean;
    failed: boolean;
    playing: Clip | null;
};

export class Clips {
    readonly app: App;
    readonly hub: Hub;
    player: Hls | null = null;
    readonly store: Store<ClipsState>;

    constructor(app: App, hub: Hub) {
        this.app = app;
        this.hub = hub;
        this.store = new Store<ClipsState>({ clips: [], loading: false, failed: false, playing: null });
    }

    static duration(milliseconds: number): string {
        const seconds = Math.round(milliseconds / 1000);

        return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
    }

    static expiry(expiresAt: string, now = Date.now()): string {
        const hours = Math.floor((Date.parse(expiresAt) - now) / 3_600_000);

        if (hours >= 48) {
            return `some em ${Math.round(hours / 24)} dias`;
        }

        if (hours >= 1) {
            return `some em ${hours} h`;
        }

        return 'some em menos de 1 h';
    }

    refresh(): void {
        if (this.app.store.state.tab !== 'clips') {
            return;
        }

        void this.load();
    }

    async load(): Promise<void> {
        if (! this.hub.user) {
            return;
        }

        this.store.set({ loading: true });

        const clips = await this.hub.attempt(() => this.hub.api.get<Clip[]>('/api/clips'));

        this.store.set(clips ? { clips, loading: false, failed: false } : { loading: false, failed: true });
    }

    forget(): void {
        this.closePlayer();
        this.store.set({ clips: [] });
    }

    update(clip: Clip): void {
        this.store.set(state => {
            const index = state.clips.findIndex(item => item.id === clip.id);

            if (index === -1) {
                return { clips: [clip, ...state.clips] };
            }

            const clips = [...state.clips];

            clips[index] = clip;

            return { clips };
        });
    }

    async download(clip: Clip): Promise<void> {
        try {
            await Tauri.invoke('open_url', { url: clip.download_url });
        } catch (failure) {
            this.app.log('clip.download.error', { clip: clip.id, message: Failure.message(failure) });
            this.app.toast(`não deu para abrir o download: ${Failure.message(failure)}`, true);
        }
    }

    play(clip: Clip): void {
        if (! Hls.isSupported() && ! document.createElement('video').canPlayType('application/vnd.apple.mpegurl')) {
            this.app.log('clip.play.unsupported', { clip: clip.id, userAgent: navigator.userAgent });
            this.app.toast('este sistema não toca o clipe: falta Media Source e HLS nativo', true);

            return;
        }

        this.store.set({ playing: clip });
    }

    attach(video: HTMLVideoElement | null): void {
        const clip = this.store.state.playing;

        if (! clip || ! video) {
            return;
        }

        const playlist = clip.playlist_url ?? '';

        this.player?.destroy();
        this.player = null;

        if (Hls.isSupported()) {
            this.player = new Hls();
            this.player.on(Hls.Events.ERROR, (eventName, data) => {
                if (! data.fatal) {
                    return;
                }

                this.app.log('clip.play.error', { clip: clip.id, type: data.type, details: data.details });
                this.app.toast(`o clipe não tocou: ${data.details}`, true);
                this.closePlayer();
            });
            this.player.loadSource(playlist);
            this.player.attachMedia(video);
        } else {
            video.onerror = () => {
                this.app.log('clip.play.error', { clip: clip.id, code: video.error?.code ?? null });
                this.app.toast('o clipe não tocou: o link pode ter vencido, feche e abra de novo', true);
                this.closePlayer();
            };
            video.src = playlist;
        }

        void video.play().catch((failure: unknown) => this.app.log('clip.play.error', { clip: clip.id, message: Failure.message(failure) }));
    }

    detach(video: HTMLVideoElement | null): void {
        this.player?.destroy();
        this.player = null;

        if (! video) {
            return;
        }

        video.pause();
        video.removeAttribute('src');
        video.load();
    }

    closePlayer(): void {
        if (! this.store.state.playing) {
            return;
        }

        this.store.set({ playing: null });
    }

    async remove(clip: Clip): Promise<void> {
        if (! await this.app.confirm(`Apagar o clipe de ${clip.streamer.name}? Não dá para desfazer.`, 'Apagar')) {
            return;
        }

        const removed = await this.hub.attempt(async () => {
            await this.hub.api.delete(`/api/clips/${clip.id}`);

            return true;
        });

        if (! removed) {
            return;
        }

        if (this.store.state.playing?.id === clip.id) {
            this.closePlayer();
        }

        this.store.set(state => ({ clips: state.clips.filter(item => item.id !== clip.id) }));
    }
}
