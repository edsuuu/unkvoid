import { SfuClient } from './SfuClient.js';

export class VoiceStage {
    constructor() {
        this.client = null;
        this.channelId = null;
        this.statsTimer = null;
        this.bytesMark = new Map();
    }

    start() {
        document.addEventListener('livewire:init', () => {
            Livewire.on('voice-join', payload => this.join(payload.channelId, payload.channelName));
            Livewire.on('voice-stop-broadcast', payload => this.moderate('stopBroadcastOf', payload.userId));
            Livewire.on('voice-kick', payload => this.moderate('kick', payload.userId));
            Livewire.on('url-changed', payload => history.replaceState({}, '', payload.url));
        });

        document.addEventListener('click', event => {
            const action = event.target.closest('[data-action]')?.dataset.action;

            if (!action) {
                return;
            }

            if (action === 'share') this.share();
            if (action === 'stop-share') this.stopShare();
            if (action === 'leave') this.leave();
            if (action === 'toggle-mic') this.toggleMicrophone();
        });
    }

    stage() {
        return document.querySelector('[data-voice-stage]');
    }

    grid() {
        return document.querySelector('[data-voice-grid]');
    }

    status(text) {
        window.dispatchEvent(new CustomEvent('voice-status', { detail: { text } }));
    }

    async join(channelId, channelName) {
        if (this.channelId === channelId) {
            return;
        }

        await this.leave();

        let credentials;

        try {
            const response = await fetch(`/api/voz/${channelId}/token`, {
                method: 'POST',
                headers: {
                    'X-CSRF-TOKEN': document.querySelector('meta[name=csrf-token]')?.content ?? '',
                    Accept: 'application/json',
                },
            });

            if (!response.ok) {
                throw new Error(`servidor recusou (${response.status})`);
            }

            credentials = await response.json();
        } catch (error) {
            this.status(`erro: ${error.message}`);

            return;
        }

        this.client = new SfuClient();
        this.client.addEventListener('newProducer', event => this.consume(event.detail));
        this.client.addEventListener('producerClosed', event => this.removeTile(event.detail.producerId));
        this.client.addEventListener('peerProducersClosed', event => this.removePeerTiles(event.detail.peerId));
        this.client.addEventListener('broadcastStopped', event => this.status(`${event.detail.by} encerrou sua transmissão`));
        this.client.addEventListener('kicked', () => { this.status('você foi removido da chamada'); this.leave(); });
        this.client.addEventListener('shareEnded', () => this.stopShare());
        this.client.addEventListener('closed', () => this.teardown());

        try {
            const joined = await this.client.connect(credentials.url, credentials.token);

            this.channelId = channelId;
            this.showStage(true);
            this.status(`em ${channelName}`);

            for (const peer of joined.peers) {
                for (const producer of peer.producers) {
                    await this.consume({ ...producer, peerId: peer.peerId, name: peer.name });
                }
            }

            this.statsTimer = setInterval(() => this.refreshStats(), 1000);
        } catch (error) {
            this.status(`não conectou: ${error.message}`);
            this.teardown();
        }
    }

    async moderate(method, targetPeerId) {
        if (!this.client) {
            return;
        }

        try {
            await this.client[method](targetPeerId);
        } catch (error) {
            this.status(`ação recusada: ${error.message}`);
        }
    }

    async consume({ producerId, name }) {
        try {
            const { consumer, source, peerId } = await this.client.consume(producerId);

            if (consumer.kind === 'audio') {
                const audio = document.createElement('audio');
                audio.srcObject = new MediaStream([consumer.track]);
                audio.autoplay = true;
                audio.dataset.tile = producerId;
                audio.dataset.peer = peerId;
                document.body.appendChild(audio);

                return;
            }

            this.addTile(producerId, peerId, name, consumer.track);
        } catch (error) {
            this.status(`erro ao receber vídeo: ${error.message}`);
        }
    }

    addTile(producerId, peerId, name, track) {
        const tile = document.createElement('figure');
        tile.className = 'm-0 flex flex-col overflow-hidden rounded-lg bg-black';
        tile.dataset.tile = producerId;
        tile.dataset.peer = peerId;

        const video = document.createElement('video');
        video.className = 'min-h-0 w-full flex-1 object-contain';
        video.srcObject = new MediaStream([track]);
        video.autoplay = true;
        video.playsInline = true;
        video.muted = true;

        const caption = document.createElement('figcaption');
        caption.className = 'bg-[#232428] px-3 py-1.5 text-xs text-[#b5bac1]';
        caption.textContent = name;

        tile.append(video, caption);
        this.grid()?.appendChild(tile);
        this.layoutGrid();
    }

    removeTile(producerId) {
        document.querySelectorAll(`[data-tile="${producerId}"]`).forEach(element => element.remove());
        this.layoutGrid();
    }

    removePeerTiles(peerId) {
        document.querySelectorAll(`[data-peer="${peerId}"]`).forEach(element => element.remove());
        this.layoutGrid();
    }

    layoutGrid() {
        const grid = this.grid();

        if (!grid) {
            return;
        }

        const count = grid.querySelectorAll('figure').length;

        grid.style.gridTemplateColumns = count > 1 ? 'repeat(2, minmax(0, 1fr))' : '1fr';
    }

    showStage(visible) {
        window.dispatchEvent(new CustomEvent('voice-state', { detail: { inCall: visible } }));
    }

    async share() {
        if (!this.client) {
            return;
        }

        try {
            const { hasAudio } = await this.client.shareScreen({
                profile: document.querySelector('[data-quality]')?.value ?? '1080',
                codec: 'h264',
                simulcast: false,
                contentHint: 'detail',
            });

            document.querySelector('[data-action="share"]')?.classList.add('hidden');
            document.querySelector('[data-action="stop-share"]')?.classList.remove('hidden');
            this.status(hasAudio ? 'compartilhando com áudio' : 'compartilhando sem áudio do sistema');
        } catch (error) {
            this.status(`não compartilhou: ${error.message}`);
        }
    }

    async stopShare() {
        await this.client?.stopShare();
        document.querySelector('[data-action="share"]')?.classList.remove('hidden');
        document.querySelector('[data-action="stop-share"]')?.classList.add('hidden');
    }

    async toggleMicrophone() {
        if (!this.client) {
            return;
        }

        const on = await this.client.toggleMicrophone();

        document.querySelector('[data-action="toggle-mic"]')?.classList.toggle('text-[#f23f43]', !on);
    }

    async refreshStats() {
        const rows = await this.client?.outboundStats();
        const element = document.querySelector('[data-voice-stats]');

        if (!element) {
            return;
        }

        if (!rows?.length) {
            element.textContent = '';

            return;
        }

        element.textContent = rows.map(row => {
            const previous = this.bytesMark.get(row.id);
            this.bytesMark.set(row.id, row.bytesSent);
            const mbps = previous ? ((row.bytesSent - previous) * 8 / 1e6).toFixed(1) : '—';

            return `${row.resolution} · ${Math.round(row.fps)}fps · ${mbps} Mbps${row.limitedBy === 'none' ? '' : ` · ${row.limitedBy}`}`;
        }).join('  |  ');
    }

    async leave() {
        if (!this.client) {
            return;
        }

        await this.client.stopShare();
        this.client.disconnect();
        this.teardown();
    }

    teardown() {
        clearInterval(this.statsTimer);
        this.client = null;
        this.channelId = null;
        this.bytesMark.clear();

        const grid = this.grid();

        if (grid) {
            grid.innerHTML = '';
        }

        document.querySelectorAll('audio[data-tile]').forEach(element => element.remove());
        this.showStage(false);
        this.status('Disponível');
    }
}
