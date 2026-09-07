import { PresenceClient } from './PresenceClient.js';
import { SfuClient } from './SfuClient.js';

const ICONS = {
    audioOn: '<svg viewBox="0 0 24 24" fill="currentColor" class="size-4"><path d="M11.38 3.08A1 1 0 0 1 12 4v16a1 1 0 0 1-1.71.71L5.59 16H3a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1h2.59l4.7-4.71a1 1 0 0 1 1.09-.21zM16.5 7.5a1 1 0 0 1 1.41 0 6 6 0 0 1 0 8.49 1 1 0 1 1-1.41-1.42 4 4 0 0 0 0-5.65 1 1 0 0 1 0-1.42z"/></svg>',
    audioOff: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="size-4"><path stroke-linecap="round" d="M11 5 6 9H3v6h3l5 4V5zM17 9l4 6M21 9l-4 6"/></svg>',
    focus: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="size-4"><rect x="3" y="5" width="18" height="14" rx="2"/><rect x="7" y="9" width="10" height="6" rx="1" fill="currentColor" stroke="none"/></svg>',
    fullscreen: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="size-4"><path stroke-linecap="round" stroke-linejoin="round" d="M4 9V5a1 1 0 0 1 1-1h4M20 9V5a1 1 0 0 0-1-1h-4M4 15v4a1 1 0 0 0 1 1h4M20 15v4a1 1 0 0 1-1 1h-4"/></svg>',
    close: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="size-4"><path stroke-linecap="round" d="M6 6l12 12M18 6L6 18"/></svg>',
};

export class VoiceStage {
    constructor() {
        this.client = null;
        this.presence = new PresenceClient();
        this.channelId = null;
        this.statsTimer = null;
        this.clockTimer = null;
        this.joinedAt = null;
        this.channelName = '';
        this.focused = null;
        this.bytesMark = new Map();
        this.presenceState = null;
        this.escapeHandler = event => {
            if (event.key !== 'Escape') {
                return;
            }

            document.querySelectorAll('[data-expanded="true"]').forEach(tile => this.collapseTile(tile));
        };
    }

    /** Chave onde fica o canal ativo, para o F5 não derrubar a pessoa da chamada. */
    static STORAGE_KEY = 'voice:channel';

    remember(channelId, channelName) {
        try {
            // Não sobrescreve com nulo: ao restaurar, a página ainda está no painel
            // inicial e o servidor não está no DOM — o valor salvo é o que vale.
            const anterior = JSON.parse(localStorage.getItem(VoiceStage.STORAGE_KEY) ?? 'null');
            const serverId = document.querySelector('[data-server-id]')?.dataset.serverId
                ?? (anterior?.channelId === channelId ? anterior.serverId : null);

            localStorage.setItem(VoiceStage.STORAGE_KEY, JSON.stringify({ channelId, channelName, serverId }));
        } catch {
            // Navegador sem storage: perde só a reconexão automática após recarregar.
        }
    }

    forget() {
        try {
            localStorage.removeItem(VoiceStage.STORAGE_KEY);
        } catch {
            // idem
        }
    }

    restore() {
        try {
            const saved = JSON.parse(localStorage.getItem(VoiceStage.STORAGE_KEY) ?? 'null');

            if (! saved?.channelId) {
                return;
            }

            void this.join(saved.channelId, saved.channelName ?? '');
        } catch {
            this.forget();
        }
    }

    watchCurrentServer() {
        const serverId = document.querySelector('[data-server-id]')?.dataset.serverId;

        if (!serverId) {
            this.presence.stop();

            return;
        }

        void this.presence.watch(serverId).catch(() => {});
    }

    /**
     * Desenha quem está em cada canal de voz a partir do que o servidor empurra —
     * inclusive para quem não entrou em canal nenhum.
     */
    renderPresence(channels) {
        this.presenceState = channels;

        document.querySelectorAll('[data-voice-members]').forEach(list => {
            const members = channels[list.dataset.voiceMembers]?.members ?? [];

            list.innerHTML = members.map(member => `
                <div class="flex items-center gap-2 rounded px-2 py-1 text-sm text-[#949ba4]">
                    <span class="flex size-6 shrink-0 items-center justify-center overflow-hidden rounded-full bg-[#5865f2] text-[10px] font-semibold text-white">
                        ${member.avatar ? `<img src="${member.avatar}" alt="" class="size-6 object-cover">` : member.name.slice(0, 2).toUpperCase()}
                    </span>
                    <span class="truncate">${member.name}</span>
                </div>
            `).join('');
        });

        this.renderChannelClocks();
    }

    /**
     * O tempo ao lado do canal vem do servidor, então quem NÃO está na chamada
     * também vê há quanto tempo ela rola.
     */
    renderChannelClocks() {
        document.querySelectorAll('[data-channel-clock]').forEach(element => {
            const presence = this.presenceState?.[element.dataset.channelClock];

            if (! presence?.members.length) {
                element.textContent = '';

                return;
            }

            const seconds = Math.max(0, Math.floor((Date.now() - presence.startedAt) / 1000));
            const parts = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60];

            element.textContent = (parts[0] ? parts : parts.slice(1))
                .map(value => String(value).padStart(2, '0'))
                .join(':');
        });
    }

    me() {
        const root = document.querySelector('[data-me]');

        return {
            name: root?.dataset.me ?? 'você',
            avatar: root?.dataset.meAvatar || null,
        };
    }

    /**
     * Coloca você embaixo do canal antes do handshake terminar. Sem isso o clique
     * parece não ter feito nada durante os segundos de conexão.
     */
    showSelfPending(channelId) {
        const list = document.querySelector(`[data-voice-members="${channelId}"]`);

        if (!list) {
            return;
        }

        const { name, avatar } = this.me();

        list.innerHTML = `
            <div class="flex animate-pulse items-center gap-2 rounded px-2 py-1 text-sm text-[#949ba4]">
                <span class="flex size-6 shrink-0 items-center justify-center overflow-hidden rounded-full bg-[#5865f2] text-[10px] font-semibold text-white">
                    ${avatar ? `<img src="${avatar}" alt="" class="size-6 object-cover">` : name.slice(0, 2).toUpperCase()}
                </span>
                <span class="truncate">${name}</span>
            </div>
        `;
    }

    start() {
        document.addEventListener('livewire:init', () => {
            Livewire.on('voice-join', payload => this.join(payload.channelId, payload.channelName));
            Livewire.on('voice-stop-broadcast', payload => this.moderate('stopBroadcastOf', payload.userId));
            Livewire.on('voice-disconnect', payload => this.moderate('disconnectPeer', payload.userId));
            Livewire.on('url-changed', payload => history.replaceState({}, '', payload.url));

            this.presence.addEventListener('presence', event => this.renderPresence(event.detail));
            setInterval(() => this.renderChannelClocks(), 1000);
            this.watchCurrentServer();

            // Observar o atributo é mais confiável que hook do Livewire: funciona
            // independente de quando o morph termina e de mudanças de versão.
            new MutationObserver(() => this.watchCurrentServer()).observe(document.body, {
                subtree: true,
                attributes: true,
                attributeFilter: ['data-server-id'],
            });

        });

        // restore() só depois dos componentes existirem: no 'livewire:init' um
        // Livewire.dispatch se perde, porque ninguém está escutando ainda.
        document.addEventListener('livewire:initialized', () => this.restore());

        document.addEventListener('change', event => {
            if (event.target.matches('[data-quality]')) {
                this.applyQuality(event.target.value);
            }
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

        this.showSelfPending(channelId);
        this.setControlsEnabled(false);
        window.dispatchEvent(new CustomEvent('voice-connecting'));

        const fetchCredentials = async () => {
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

            return response.json();
        };

        let credentials;

        try {
            credentials = await fetchCredentials();
        } catch (error) {
            this.status(`erro: ${error.message}`);

            return;
        }

        this.client = new SfuClient();
        this.client.addEventListener('newProducer', event => this.consume(event.detail));
        this.client.addEventListener('producerClosed', event => this.removeTile(event.detail.producerId));
        this.client.addEventListener('peerProducersClosed', event => this.removePeerTiles(event.detail.peerId));
        this.client.addEventListener('broadcastStopped', event => this.status(`${event.detail.by} encerrou sua transmissão`));
        this.client.addEventListener('disconnected', event => {
            this.status(`${event.detail.by} tirou você da chamada`);
            this.leave();
        });
        this.client.addEventListener('shareEnded', () => this.stopShare());
        this.client.addEventListener('closed', () => this.teardown());
        this.client.addEventListener('reconnecting', event => {
            this.status(`reconectando… (tentativa ${event.detail.attempt})`);
            window.dispatchEvent(new CustomEvent('voice-connecting'));
        });
        this.client.addEventListener('reconnected', event => {
            this.status(event.detail.resumed ? `em ${this.channelName}` : `em ${this.channelName} (republicado)`);
            window.dispatchEvent(new CustomEvent('voice-state', {
                detail: { inCall: true, channelName: this.channelName, channelId: this.channelId },
            }));
        });
        this.client.addEventListener('peersChanged', () => this.renderMembers());
        this.client.addEventListener('replaced', event => this.status(event.detail.reason));

        try {
            const joined = await this.client.connect(
                credentials.url,
                async () => (await fetchCredentials()).token,
            );

            this.channelId = channelId;
            this.channelName = channelName;
            this.showStage(true, channelName);
            this.status(`em ${channelName}`);
            this.renderMembers();

            for (const peer of joined.peers) {
                for (const producer of peer.producers) {
                    await this.consume({ ...producer, peerId: peer.peerId, name: peer.name });
                }
            }

            this.startClock();
            this.setControlsEnabled(true);
            this.remember(channelId, channelName);
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
        bar.className = 'flex items-center gap-1 bg-[#232428] px-3 py-1.5 text-xs text-[#b5bac1]';
        const button = (action, title, icon, danger = false) => `
            <button type="button" data-tile-action="${action}" data-tile-id="${producerId}" data-peer="${peerId}"
                title="${title}"
                class="flex cursor-pointer items-center justify-center rounded p-1 text-[#b5bac1] transition-colors hover:bg-[#3f4147] ${danger ? 'hover:text-[#f23f43]' : 'hover:text-white'}">${icon}</button>
        `;

        bar.innerHTML = `
            <span class="truncate">${name}</span>
            <span class="flex-1"></span>
            ${button('mute', 'Mutar o áudio desta transmissão', ICONS.audioOn)}
            ${button('focus', 'Ver só esta (esconde as outras)', ICONS.focus)}
            ${button('fullscreen', 'Tela cheia', ICONS.fullscreen)}
            ${button('close', 'Parar de assistir (libera banda)', ICONS.close, true)}
        `;

        tile.append(video, bar);
        this.grid()?.appendChild(tile);
        this.layoutGrid();
    }

    /**
     * Tenta a tela cheia nativa; se o navegador recusar (exige gesto do usuário e
     * nem todo contexto permite), cai para um modo expandido em CSS, que sempre
     * funciona. Esc sai dos dois.
     */
    async toggleFullscreen(tile) {
        if (document.fullscreenElement) {
            await document.exitFullscreen().catch(() => {});

            return;
        }

        if (tile.dataset.expanded === 'true') {
            this.collapseTile(tile);

            return;
        }

        try {
            await tile.requestFullscreen();
        } catch {
            this.expandTile(tile);
        }
    }

    expandTile(tile) {
        tile.dataset.expanded = 'true';
        tile.classList.add('fixed', 'inset-0', 'z-50', 'rounded-none');
        document.addEventListener('keydown', this.escapeHandler);
    }

    collapseTile(tile) {
        delete tile.dataset.expanded;
        tile.classList.remove('fixed', 'inset-0', 'z-50', 'rounded-none');
        document.removeEventListener('keydown', this.escapeHandler);
    }

    setControlsEnabled(enabled) {
        for (const action of ['share', 'stop-share', 'toggle-mic']) {
            const button = document.querySelector(`[data-action="${action}"]`);

            if (button) {
                button.disabled = ! enabled;
                button.classList.toggle('opacity-50', ! enabled);
                button.classList.toggle('cursor-not-allowed', ! enabled);
            }
        }
    }

    async applyQuality(profile) {
        if (! this.client?.producers.has('screen')) {
            return;
        }

        try {
            await this.client.changeQuality(profile);
            this.status(`qualidade em ${profile}p`);
        } catch (error) {
            this.status(`não trocou a qualidade: ${error.message}`);
        }
    }

    async tileAction(action, button) {
        if (action === 'mute') {
            const audio = document.querySelector(`audio[data-peer="${button.dataset.peer}"]`);

            if (!audio) {
                this.status('esta transmissão não tem áudio');

                return;
            }

            audio.muted = !audio.muted;
            button.innerHTML = audio.muted ? ICONS.audioOff : ICONS.audioOn;
            button.classList.toggle('text-[#f23f43]', audio.muted);

            return;
        }

        const tile = document.querySelector(`[data-tile="${button.dataset.tileId}"]`);

        if (action === 'focus') {
            this.focused = this.focused === button.dataset.tileId ? null : button.dataset.tileId;
            this.layoutGrid();

            return;
        }

        if (action === 'fullscreen' && tile) {
            await this.toggleFullscreen(tile);

            return;
        }

        if (action === 'close' && tile) {
            await this.client.pauseConsumer(tile.dataset.consumer).catch(() => {});
            tile.remove();
            this.layoutGrid();
        }
    }

    removeTile(producerId) {
        document.querySelectorAll(`[data-tile="${producerId}"][data-expanded="true"]`)
            .forEach(tile => this.collapseTile(tile));
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
            document.querySelector('[data-voice-empty]')?.style.setProperty('display', 'none');

            return;
        }

        tiles.forEach(tile => tile.classList.remove('hidden'));

        const columns = tiles.length <= 1 ? 1 : tiles.length <= 4 ? 2 : 3;

        grid.style.gridTemplateColumns = `repeat(${columns}, minmax(0, 1fr))`;
        grid.style.gridAutoRows = tiles.length <= 2 ? '1fr' : 'minmax(0, 1fr)';

        const empty = document.querySelector('[data-voice-empty]');

        if (empty) {
            empty.style.display = tiles.length ? 'none' : '';
        }
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

    async announceLeave() {
        await this.client?.leaveRoom();
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
        // Clicar antes do handshake terminar deixava o erro invisível: o join
        // completava logo depois e sobrescrevia a mensagem de falha.
        if (! this.client?.sendTransport) {
            this.status('espere terminar de conectar para compartilhar');

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
        if (! this.client?.sendTransport) {
            this.status('espere terminar de conectar');

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
        this.forget();

        if (!this.client) {
            return;
        }

        await this.client.stopShare();
        await this.announceLeave();
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
        this.setControlsEnabled(true);
        this.showStage(false);
        this.status('Disponível');
    }
}
