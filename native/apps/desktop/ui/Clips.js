import Hls from 'hls.js/light';

const el = id => document.getElementById(id);

const { invoke } = window.__TAURI__.core;

/**
 * A aba Clipes: os clipes que eu fiz, o mini player e o apagar.
 *
 * A Transmissão continua montada atrás dela: trocar de aba só esconde o
 * `#broadcast-view`, então sala, voz e palco seguem exatamente como estavam.
 */
export class Clips {
    constructor(app, hub) {
        this.app = app;
        this.hub = hub;
        this.clips = [];
        this.player = null;
        this.playing = null;

        el('tab-broadcast').onclick = () => this.showTab('broadcast');
        el('tab-clips').onclick = () => this.showTab('clips');
        el('clip-player-close').onclick = () => this.closePlayer();
    }

    static duration(milliseconds) {
        const seconds = Math.round(milliseconds / 1000);

        return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
    }

    static expiry(expiresAt, now = Date.now()) {
        const hours = Math.floor((Date.parse(expiresAt) - now) / 3_600_000);

        if (hours >= 48) {
            return `some em ${Math.round(hours / 24)} dias`;
        }

        if (hours >= 1) {
            return `some em ${hours} h`;
        }

        return 'some em menos de 1 h';
    }

    /**
     * O painel de entrar é um só e muda de aba junto: Google e e-mail continuam sendo os
     * do `Hub.wireEntry`, sem uma segunda cópia para envelhecer. O palco que ficou atrás
     * para de decodificar, como com a janela minimizada.
     */
    showTab(tab) {
        const clips = tab === 'clips';

        el('broadcast-view').hidden = clips;
        el('clips-view').hidden = ! clips;
        el('tab-broadcast').classList.toggle('pill-on', ! clips);
        el('tab-clips').classList.toggle('pill-on', clips);
        el('tab-broadcast').setAttribute('aria-selected', String(! clips));
        el('tab-clips').setAttribute('aria-selected', String(clips));
        (clips ? el('clips-login-slot') : el('entry-cards')).append(el('login-panel'));
        this.app.paintWatching();

        if (! clips) {
            this.closePlayer();

            return;
        }

        void this.load();
    }

    /** Só com a aba aberta: quem nunca a abriu não paga a busca. */
    refresh() {
        if (el('clips-view').hidden) {
            return;
        }

        void this.load();
    }

    async load() {
        if (! this.hub.user) {
            return;
        }

        const clips = await this.hub.attempt(() => this.hub.api.get('/api/clips'));

        if (! clips) {
            return;
        }

        this.clips = clips;
        this.draw();
    }

    forget() {
        this.closePlayer();
        this.clips = [];
        this.draw();
    }

    /** `ClipUpdated` e o 202 do Clipar: troca o cartão que existe, ou põe o novo no topo. */
    update(clip) {
        const index = this.clips.findIndex(item => item.id === clip.id);

        if (index === -1) {
            this.clips.unshift(clip);
        } else {
            this.clips[index] = clip;
        }

        this.draw();
    }

    draw() {
        el('clip-list').replaceChildren(...this.clips.map(clip => this.card(clip)));
        el('clip-empty').hidden = this.clips.length > 0;
    }

    card(clip) {
        const card = document.createElement('article');

        card.className = 'card-sm flex flex-col overflow-hidden';
        card.dataset.clip = clip.id;
        card.dataset.status = clip.status;
        card.innerHTML = '<div class="flex aspect-video items-center justify-center overflow-hidden bg-black">'
            + '<img class="size-full object-cover" data-clip-image alt="" loading="lazy">'
            + '<span class="flex items-center gap-2 text-xs text-ink-soft" data-clip-processing><span class="size-4 animate-spin rounded-full border-2 border-white/20 border-t-brand"></span>Salvando o clipe…</span>'
            + '<span class="px-4 text-center text-xs text-danger" data-clip-failed>Não deu para salvar este clipe.</span>'
            + '</div>'
            + '<div class="flex flex-col gap-1 p-3">'
            + '<p class="truncate text-sm font-semibold" data-clip-streamer></p>'
            + '<p class="truncate text-xs text-ink-soft" data-clip-where></p>'
            + '<p class="label-mono" data-clip-when></p>'
            + '<div class="mt-2 flex gap-1.5">'
            + '<button class="btn-primary px-3 py-1.5 text-xs" data-clip-play type="button">Assistir</button>'
            + '<button class="btn-ghost px-3 py-1.5 text-xs" data-clip-download type="button">Baixar</button>'
            + '<span class="flex-1"></span>'
            + '<button class="btn-ghost px-3 py-1.5 text-xs text-danger" data-clip-delete type="button">Apagar</button>'
            + '</div>'
            + '</div>';

        const image = card.querySelector('[data-clip-image]');
        const play = card.querySelector('[data-clip-play]');

        image.hidden = ! clip.thumbnail_url;

        if (clip.thumbnail_url) {
            image.src = clip.thumbnail_url;
        }

        card.querySelector('[data-clip-processing]').hidden = clip.status !== 'processing';
        card.querySelector('[data-clip-failed]').hidden = clip.status !== 'failed';
        card.querySelector('[data-clip-streamer]').textContent = clip.streamer.name;
        card.querySelector('[data-clip-where]').textContent = `${clip.server_name} › ${clip.channel_name}`;
        card.querySelector('[data-clip-when]').textContent = [
            new Date(clip.created_at).toLocaleString('pt-BR', { dateStyle: 'short', timeStyle: 'short' }),
            clip.duration_ms ? Clips.duration(clip.duration_ms) : null,
            Clips.expiry(clip.expires_at),
        ].filter(Boolean).join(' · ');
        play.hidden = ! (clip.status === 'ready' && clip.playlist_url);
        play.onclick = () => this.play(clip);
        card.querySelector('[data-clip-download]').hidden = ! clip.download_url;
        card.querySelector('[data-clip-download]').onclick = () => void this.download(clip);
        card.querySelector('[data-clip-delete]').onclick = () => void this.remove(clip);

        return card;
    }

    /**
     * Pelo navegador do sistema: na janela do Tauri um `<a download>` para outra origem
     * não baixa de forma confiável, e o `download_url` já vem assinado com `attachment`.
     */
    async download(clip) {
        try {
            await invoke('open_url', { url: clip.download_url });
        } catch (failure) {
            this.app.log('clip.download.error', { clip: clip.id, message: failure.message ?? String(failure) });
            this.app.toast(`não deu para abrir o download: ${failure.message ?? failure}`, true);
        }
    }

    /**
     * hls.js onde há Media Source; HLS nativo onde só o motor sabe tocar. A
     * `playlist_url` já vem assinada, então nenhum cabeçalho vai junto.
     */
    play(clip) {
        const video = el('clip-video');
        const hls = Hls.isSupported();

        this.closePlayer();

        if (! hls && ! video.canPlayType('application/vnd.apple.mpegurl')) {
            this.app.log('clip.play.unsupported', { clip: clip.id, userAgent: navigator.userAgent });
            this.app.toast('este sistema não toca o clipe: falta Media Source e HLS nativo', true);

            return;
        }

        this.playing = clip.id;
        el('clip-player-title').textContent = `${clip.streamer.name} · ${clip.server_name} › ${clip.channel_name}`;
        el('clip-player').hidden = false;

        if (hls) {
            this.player = new Hls();
            this.player.on(Hls.Events.ERROR, (eventName, data) => {
                if (! data.fatal) {
                    return;
                }

                this.app.log('clip.play.error', { clip: clip.id, type: data.type, details: data.details });
                this.app.toast(`o clipe não tocou: ${data.details}`, true);
                this.closePlayer();
            });
            this.player.loadSource(clip.playlist_url);
            this.player.attachMedia(video);
        } else {
            video.src = clip.playlist_url;
        }

        void video.play().catch(failure => this.app.log('clip.play.error', { clip: clip.id, message: failure.message ?? String(failure) }));
    }

    closePlayer() {
        if (! this.playing) {
            return;
        }

        const video = el('clip-video');

        this.player?.destroy();
        this.player = null;
        this.playing = null;
        el('clip-player').hidden = true;
        video.pause();
        video.removeAttribute('src');
        video.load();
    }

    async remove(clip) {
        if (! confirm(`Apagar o clipe de ${clip.streamer.name}? Não dá para desfazer.`)) {
            return;
        }

        const removed = await this.hub.attempt(async () => {
            await this.hub.api.delete(`/api/clips/${clip.id}`);

            return true;
        });

        if (! removed) {
            return;
        }

        if (this.playing === clip.id) {
            this.closePlayer();
        }

        this.clips = this.clips.filter(item => item.id !== clip.id);
        this.draw();
    }
}
