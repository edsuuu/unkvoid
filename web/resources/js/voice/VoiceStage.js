import { SfuClient } from './SfuClient.js';

export class VoiceStage {
    constructor() {
        this.client = null;
        this.channelId = null;
        this.statsTimer = null;
        this.clockTimer = null;
        this.joinedAt = null;
        this.focused = null;
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
            const tileButton = event.target.closest('[data-tile-action]');

            if (tileButton) {
                this.tileAction(tileButton.dataset.tileAction, tileButton);

                return;
            }

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
        this.client.addEventListener('peersChanged', () => this.renderMembers());
        this.client.addEventListener('replaced', event => this.status(event.detail.reason));

        try {
            const joined = await this.client.connect(credentials.url, credentials.token);

            this.channelId = channelId;
            this.showStage(true, channelName);
            this.status(`em ${channelName}`);
            this.renderMembers();

            for (const peer of joined.peers) {
                for (const producer of peer.producers) {
                    await this.consume({ ...producer, peerId: peer.peerId, name: peer.name });
                }
            }

            this.startClock();
            this.statsTimer = setInterval(() => this.refreshStats(), 1000);
        } catch (error) {
            this.teardown();
            this.status(`não conectou: ${error.message}`);
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

            this.addTile(producerId, peerId, name, consumer.track, consumer.id);
        } catch (error) {
            this.status(`erro ao receber vídeo: ${error.message}`);
        }
    }

    addTile(producerId, peerId, name, track, consumerId) {
        const tile = document.createElement('figure');
        tile.className = 'group m-0 flex flex-col overflow-hidden rounded-lg bg-black';
        tile.dataset.tile = producerId;
        tile.dataset.peer = peerId;
        tile.dataset.consumer = consumerId;

        const video = document.createElement('video');
        video.className = 'min-h-0 w-full flex-1 object-contain';
        video.srcObject = new MediaStream([track]);
        video.autoplay = true;
        video.playsInline = true;
        video.muted = true;

        const bar = document.createElement('figcaption');
        bar.className = 'flex items-center gap-2 bg-[#232428] px-3 py-1.5 text-xs text-[#b5bac1]';
        bar.innerHTML = `
            <span class="truncate">${name}</span>
            <span class="flex-1"></span>
            <button type="button" data-tile-action="mute" data-peer="${peerId}"
                title="Mutar o áudio desta transmissão"
                class="cursor-pointer rounded px-1.5 py-0.5 hover:bg-[#35373c] hover:text-white">🔊</button>
            <button type="button" data-tile-action="focus" data-tile-id="${producerId}"
                title="Focar nesta transmissão"
                class="cursor-pointer rounded px-1.5 py-0.5 hover:bg-[#35373c] hover:text-white">⛶</button>
            <button type="button" data-tile-action="close" data-tile-id="${producerId}"
                title="Parar de assistir (libera banda)"
                class="cursor-pointer rounded px-1.5 py-0.5 hover:bg-[#35373c] hover:text-[#f23f43]">✕</button>
        `;

        tile.append(video, bar);
        this.grid()?.appendChild(tile);
        this.layoutGrid();
    }

    async tileAction(action, button) {
        if (action === 'mute') {
            const audio = document.querySelector(`audio[data-peer="${button.dataset.peer}"]`);

            if (!audio) {
                this.status('esta transmissão não tem áudio');

                return;
            }

            audio.muted = !audio.muted;
            button.textContent = audio.muted ? '🔇' : '🔊';

            return;
        }

        const tile = document.querySelector(`[data-tile="${button.dataset.tileId}"]`);

        if (action === 'focus') {
            this.focused = this.focused === button.dataset.tileId ? null : button.dataset.tileId;
            this.layoutGrid();

            return;
        }

        if (action === 'close' && tile) {
            await this.client.pauseConsumer(tile.dataset.consumer).catch(() => {});
            tile.remove();
            this.layoutGrid();
        }
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

        const tiles = [...grid.querySelectorAll('figure')];

        if (this.focused && grid.querySelector(`[data-tile="${this.focused}"]`)) {
            grid.style.gridTemplateColumns = '1fr';
            tiles.forEach(tile => tile.classList.toggle('hidden', tile.dataset.tile !== this.focused));

            return;
        }

        tiles.forEach(tile => tile.classList.remove('hidden'));
        grid.style.gridTemplateColumns = tiles.length > 1 ? 'repeat(2, minmax(0, 1fr))' : '1fr';
    }

    showStage(visible, channelName = null) {
        window.dispatchEvent(new CustomEvent('voice-state', {
            detail: { inCall: visible, channelName, channelId: visible ? this.channelId : '' },
        }));
    }

    renderMembers() {
        const list = document.querySelector(`[data-voice-members="${this.channelId}"]`);

        if (!list) {
            return;
        }

        list.innerHTML = [...this.client.peers.values()].map(peer => `
            <div class="flex items-center gap-2 rounded px-2 py-1 text-sm ${peer.sharing ? 'text-[#23a55a]' : 'text-[#949ba4]'}">
                <span class="flex size-6 shrink-0 items-center justify-center overflow-hidden rounded-full bg-[#5865f2] text-[10px] font-semibold text-white">
                    ${peer.avatar ? `<img src="${peer.avatar}" alt="" class="size-6 object-cover">` : peer.name.slice(0, 2).toUpperCase()}
                </span>
                <span class="truncate">${peer.name}</span>
                ${peer.sharing ? `<span title="compartilhando a tela" class="ml-auto shrink-0 rounded bg-[#23a55a] px-1 py-0.5 text-[10px] font-bold uppercase text-white">ao vivo</span>` : ''}
            </div>
        `).join('');
    }

    startClock() {
        this.joinedAt = Date.now();
        this.clockTimer = setInterval(() => {
            const seconds = Math.floor((Date.now() - this.joinedAt) / 1000);
            const parts = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60];
            const label = (parts[0] ? parts : parts.slice(1))
                .map(value => String(value).padStart(2, '0'))
                .join(':');

            window.dispatchEvent(new CustomEvent('voice-clock', { detail: { label } }));
        }, 1000);
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
        clearInterval(this.clockTimer);
        this.focused = null;

        const list = document.querySelector(`[data-voice-members="${this.channelId}"]`);

        if (list) {
            list.innerHTML = '';
        }

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
