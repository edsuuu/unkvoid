import { SfuClient } from './SfuClient.js';

const { invoke } = window.__TAURI__.core;

const el = id => document.getElementById(id);

/**
 * Um canal de voz: mic, câmera, ensurdecer e a barra embaixo da lista de canais.
 *
 * Os cartões, o consumo e a tela compartilhada são os da sala anônima — o `App` é quem
 * os desenha; aqui só entra o que a sala não tem: publicar mic e câmera, e o token que
 * o Laravel dá para entrar.
 *
 * Windows e macOS publicam pelo `getUserMedia` e pelo transporte de envio do
 * mediasoup-client. Linux publica pelo Rust como RTP puro, igual à tela.
 */
export class Voice {
    /** DTX cala o silêncio e FEC recupera pacote perdido sem pedir de novo: voz, não música. */
    static MIC_OPTIONS = { codecOptions: { opusDtx: true, opusFec: true } };

    constructor(app, hub) {
        this.app = app;
        this.hub = hub;
        this.channel = null;
        this.muted = false;
        this.deafened = false;
        this.serverMuted = false;
        this.micProducerId = null;
        this.micTrack = null;
        this.cameraProducerId = null;
        this.cameraTrack = null;

        /** O que o token deixa publicar (`speak`, `video`, `stream`). Vem do `join`. */
        this.can = [];

        /** Sobe a cada `join`: um que ficou para trás não pode mexer no canal atual. */
        this.joinTicket = 0;

        el('voice-mute').onclick = () => void this.toggleMute();
        el('voice-deafen').onclick = () => void this.toggleDeafen();
        el('voice-camera').onclick = () => void this.toggleCamera();
        el('voice-share').onclick = () => void this.app.openShareModal();
        el('voice-stop').onclick = () => void this.app.stopSharing();
        el('voice-leave').onclick = () => void this.leave();
        el('voice-clip').onclick = () => {
            el('voice-clip-list').hidden = ! el('voice-clip-list').hidden;
            this.paintClip();
        };
    }

    /** No Linux mic e câmera passam pelo Rust: o WebKitGTK não tem `getUserMedia`. */
    native() {
        return this.app.constructor.isLinux();
    }

    /** Pelo token, não pelos bits do canal: mutado pelo servidor perde o `speak` lá. */
    allowed(grant) {
        return this.can.includes(grant);
    }

    canStream() {
        return this.allowed('stream');
    }

    /** Entra com o microfone mutado: abrir a voz não põe no ar nada que a pessoa não escolheu. */
    async join(channel) {
        if (this.channel?.id === channel.id) {
            return;
        }

        if (this.channel) {
            await this.leave();
        }

        const ticket = ++this.joinTicket;

        this.channel = channel;
        this.muted = true;
        this.can = [];
        this.serverMuted = Boolean(this.hub.me()?.server_mute);
        this.mountStage();
        this.paintBar();

        try {
            const sfu = new SfuClient();

            sfu.addEventListener('reconnected', event => {
                this.can = event.detail.can ?? this.can;
                this.paintBar();

                if (! event.detail.resumed) {
                    void this.republish();
                }
            });
            sfu.addEventListener('serverMuted', event => void this.applyServerMute(event.detail.muted));
            sfu.addEventListener('kicked', () => void this.leave());
            sfu.addEventListener('replaced', () => void this.leave());
            sfu.addEventListener('peersChanged', () => this.hub.syncVoiceSources());

            await this.app.enterRoom(sfu, () => this.identity(channel), async joined => {
                if (ticket !== this.joinTicket) {
                    return;
                }

                this.can = joined.can ?? [];
                this.paintBar();
                await this.startMic();
            });
        } catch (failure) {
            this.app.log('voice.join.error', { channel: channel.id, message: failure.message ?? String(failure) });
            this.app.toast(`não deu para entrar na voz: ${failure.message ?? failure}`, true);

            if (ticket === this.joinTicket) {
                await this.leave();
            }
        }
    }

    /**
     * O token vale 60 s, então é pedido antes de CADA `join` — inclusive nas
     * reconexões. Recusado, não há o que reconectar: a permissão sumiu (403) ou a
     * sessão (401).
     */
    async identity(channel) {
        try {
            return { token: (await this.hub.api.post(`/api/channels/${channel.id}/voice/token`)).token };
        } catch (failure) {
            if (failure.status === 401 || failure.status === 403) {
                this.app.sfu?.disconnect();
                await this.leave();
            }

            if (failure.status === 401) {
                await this.hub.logout();
            }

            throw failure;
        }
    }

    async leave() {
        const channel = this.channel;

        if (! channel) {
            return;
        }

        // Limpa antes de esperar: `kicked` e `closed` chegam juntos e os dois chamam aqui.
        this.channel = null;
        this.app.log('voice.leave', { channel: channel.id });
        await this.stopCamera().catch(failure => this.app.log('voice.camera.stop.error', { message: failure.message ?? String(failure) }));
        await this.stopMic().catch(failure => this.app.log('voice.mic.stop.error', { message: failure.message ?? String(failure) }));
        await this.app.tearDownMedia();
        this.can = [];
        this.deafened = false;
        this.serverMuted = false;
        this.unmountStage();
        this.paintBar();
        this.hub.drawCenter();
    }

    /**
     * O palco é o mesmo da sala anônima, movido para o centro do modo servidor. Mover
     * em vez de copiar: os cartões, o foco e a tela cheia continuam sendo um código só.
     */
    mountStage() {
        el('stage-host').append(el('stage'), el('stage-empty'));
        el('stage-empty').querySelectorAll('p')[1].hidden = true;
        el('stage-empty').querySelector('p').textContent = 'Ninguém está mostrando nada. Compartilhe a tela ou ligue a câmera pela barra da voz.';
    }

    unmountStage() {
        el('room').append(el('stage'), el('stage-empty'));
        el('stage-empty').querySelectorAll('p')[1].hidden = false;
    }

    paintBar() {
        const mute = el('voice-mute');
        const voiceless = ! this.allowed('speak');

        el('voice-bar').hidden = ! this.channel;
        el('voice-channel-name').textContent = this.channel?.name ?? '';
        mute.disabled = this.serverMuted || voiceless;
        mute.textContent = this.serverMuted ? '🔇 Mutado pelo servidor' : voiceless ? '🔇 Sem voz' : this.muted ? '🔇 Mudo' : '🎙️ Mic';
        mute.title = this.serverMuted
            ? 'Um moderador mutou você neste servidor'
            : voiceless ? 'Você não tem permissão para falar neste canal' : this.muted ? 'Desmutar o microfone' : 'Mutar o microfone';
        el('voice-deafen').textContent = this.deafened ? '🙉 Surdo' : '🎧 Áudio';
        el('voice-deafen').title = this.deafened ? 'Voltar a ouvir' : 'Ensurdecer: não ouvir ninguém';
        el('voice-camera').hidden = ! this.allowed('video');
        el('voice-camera').textContent = this.cameraProducerId ? '📷 Desligar' : '📷 Câmera';
        el('voice-share').hidden = ! this.canStream() || this.app.sharing;
        el('voice-stop').hidden = ! this.app.sharing;
        this.paintClip();
    }

    /**
     * Quem compartilha tela no canal de voz atual, eu inclusive. O SFU nomeia cada pessoa
     * por `user:<id>`, o mesmo elo do `Hub.syncVoiceSources`; o meu próprio producer não
     * volta como `newProducer`, então eu entro pelo `app.sharing`.
     */
    streamers() {
        const streamers = [];

        if (! this.channel) {
            return streamers;
        }

        for (const peer of this.app.sfu?.peers?.values() ?? []) {
            if (peer.self && this.app.sharing) {
                streamers.push({ userId: this.hub.user.id, name: `${this.hub.user.name} (você)` });
            } else if (! peer.self && peer.sharing && peer.userId?.startsWith('user:')) {
                streamers.push({ userId: Number(peer.userId.slice('user:'.length)), name: peer.name });
            }
        }

        return streamers;
    }

    paintClip() {
        const streamers = this.streamers();

        el('voice-clip').hidden = ! streamers.length;

        if (! streamers.length) {
            el('voice-clip-list').hidden = true;
        }

        el('voice-clip-streamers').replaceChildren(...streamers.map(streamer => {
            const button = document.createElement('button');

            button.type = 'button';
            button.className = 'row-item mt-1 w-full cursor-pointer text-left text-[13px]';
            button.textContent = streamer.name;
            button.onclick = () => void this.clip(streamer);

            return button;
        }));
    }

    /** O 202 já volta como clipe `processing`: a aba Clipes o mostra sem esperar o evento. */
    async clip(streamer) {
        const channel = this.channel;

        el('voice-clip-list').hidden = true;

        const created = await this.hub.attempt(() => this.hub.api.post(`/api/channels/${channel.id}/clips`, { user_id: streamer.userId }));

        if (! created) {
            return;
        }

        this.app.toast('Clipando os últimos 5 min — vai aparecer na aba Clipes');
        this.hub.clips.update(created);
    }

    /**
     * Publica pelo Rust: o RTP puro sobe para a porta que o servidor devolve.
     *
     * O `use_sfu` vai toda vez, e não só na primeira: o servidor pode ter recriado o
     * transporte, e o Rust já ignora o pedido quando o destino é o mesmo.
     */
    async publishNative(source) {
        const offer = await invoke('sfu_offer', { source });
        const producer = await this.app.sfu.request('producePlain', {
            kind: source === 'mic' ? 'audio' : 'video',
            source,
            ...offer,
        });

        await invoke('use_sfu', {
            address: `${producer.ip}:${producer.port}`,
            serverKey: producer.srtpParameters?.keyBase64 ?? null,
        });

        return producer.producerId;
    }

    async startMic() {
        if (! this.allowed('speak') || this.micProducerId) {
            return;
        }

        // Calado antes de publicar: pausar só depois deixaria vazar a ida e volta do
        // `produce` inteira de áudio de quem entrou mutado.
        if (this.native()) {
            await invoke('start_voice');
            await invoke('set_voice_muted', { muted: this.muted || this.serverMuted });
            this.micProducerId = await this.publishNative('mic');
        } else {
            this.micTrack ??= (await navigator.mediaDevices.getUserMedia({
                audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, latency: 0.01, channelCount: 1 },
            })).getAudioTracks()[0];
            this.micTrack.enabled = ! (this.muted || this.serverMuted);
            this.micProducerId = (await this.app.sfu.produce(this.micTrack, 'mic', Voice.MIC_OPTIONS)).id;
        }

        if (this.muted || this.serverMuted) {
            await this.applyMute();
        }

        this.app.log('voice.mic.started', { producerId: this.micProducerId, native: this.native() });
    }

    async stopMic() {
        if (this.micProducerId) {
            await this.app.sfu?.closeProducer(this.micProducerId);
            this.micProducerId = null;
        }

        this.micTrack?.stop();
        this.micTrack = null;

        if (this.native()) {
            await invoke('stop_voice').catch(failure => this.app.log('voice.stop.error', { message: failure.message ?? String(failure) }));
        }
    }

    async toggleMute() {
        if (this.serverMuted || ! this.allowed('speak')) {
            return;
        }

        this.muted = ! this.muted;
        this.paintBar();
        await this.hub.attempt(() => this.applyMute());
    }

    /** Pausa no servidor: os outros recebem `producerPaused`, e o Rust para de capturar. */
    async applyMute() {
        const muted = this.muted || this.serverMuted;

        if (this.micProducerId) {
            // Mutado pelo servidor o producer já está pausado lá, e retomar dá 403: só
            // o lado de cá para de codificar.
            if (this.serverMuted) {
                this.app.sfu.producers.get(this.micProducerId)?.pause();
            } else {
                await this.app.sfu[muted ? 'pauseProducer' : 'resumeProducer'](this.micProducerId);
            }
        }

        if (this.native()) {
            await invoke('set_voice_muted', { muted });
        }
    }

    /**
     * Um moderador mutou (ou desmutou) esta conta. Mutado antes de entrar, o token veio
     * sem `speak` e não há producer: desmutar aí é entrar de novo com um token novo.
     */
    async applyServerMute(muted) {
        this.serverMuted = muted;
        this.paintBar();

        if (! muted && ! this.micProducerId && ! this.allowed('speak') && this.channel) {
            const channel = this.channel;

            await this.leave();
            await this.join(channel);

            return;
        }

        await this.hub.attempt(() => this.applyMute());
    }

    /**
     * Ensurdecer é parar de receber, não só de ouvir: cada `<audio>` cala e cada
     * consumer de ÁUDIO pausa no servidor. O vídeo segue: pausá-lo custaria um
     * quadro-chave inteiro para voltar a ver.
     */
    async toggleDeafen() {
        this.deafened = ! this.deafened;
        this.app.deafened = this.deafened;
        this.paintBar();

        // O mic segue o ensurdecer; o áudio da tela só cala junto — voltar é escolha
        // de quem assiste, pelo botão do cartão.
        for (const audio of document.querySelectorAll('audio[data-remote]')) {
            if (audio.dataset.source === 'mic') {
                audio.muted = this.deafened;
            } else if (this.deafened) {
                audio.muted = true;
            }
        }

        for (const [producerId, key] of this.app.nativeWatching) {
            if (key.endsWith('/mic')) {
                await invoke('watch_mute', { producerId, muted: this.deafened })
                    .catch(failure => this.app.log('voice.deafen.error', { producerId, message: failure.message ?? String(failure) }));
            }
        }

        const sfu = this.app.sfu;

        for (const [consumerId, peerId] of sfu?.consumerPeers ?? []) {
            if (sfu.consumers.get(consumerId)?.kind === 'audio' && ! this.app.pausedPeers.has(peerId)) {
                await sfu.tolerate(this.deafened ? 'pauseConsumer' : 'resumeConsumer', { consumerId });
            }
        }
    }

    async toggleCamera() {
        await this.hub.attempt(() => this.cameraProducerId ? this.stopCamera() : this.startCamera());
        this.paintBar();
    }

    async startCamera() {
        if (! this.allowed('video')) {
            return;
        }

        if (this.native()) {
            const cameras = await invoke('list_cameras');

            if (! cameras.length) {
                throw new Error('nenhuma câmera encontrada');
            }

            // ponytail: a primeira câmera. Um seletor entra quando alguém tiver duas.
            await invoke('start_camera', { device: cameras[0].id });
            this.cameraProducerId = await this.publishNative('camera');

            return;
        }

        this.cameraTrack = (await navigator.mediaDevices.getUserMedia({
            video: { width: 640, height: 360, frameRate: 30 },
        })).getVideoTracks()[0];
        this.cameraProducerId = (await this.app.sfu.produce(this.cameraTrack, 'camera')).id;
        this.app.showScreen(this.app.constructor.cameraKey(this.app.sfu.peerId), new MediaStream([this.cameraTrack]), 'camera');
    }

    async stopCamera() {
        if (this.cameraProducerId) {
            await this.app.sfu?.closeProducer(this.cameraProducerId);
            this.cameraProducerId = null;
        }

        this.cameraTrack?.stop();
        this.cameraTrack = null;

        if (this.app.sfu?.peerId) {
            this.app.showScreen(this.app.constructor.cameraKey(this.app.sfu.peerId), null);
        }

        if (this.native()) {
            await invoke('stop_camera').catch(failure => this.app.log('voice.camera.stop.error', { message: failure.message ?? String(failure) }));
        }
    }

    /**
     * A sessão do SFU é outra: os producers morreram lá, a captura continua aqui.
     * Publica mic e câmera de novo; a tela o `App.afterReconnect` já republica.
     */
    async republish() {
        const hadMic = Boolean(this.micProducerId) && this.allowed('speak');
        const hadCamera = Boolean(this.cameraProducerId) && this.allowed('video');

        this.micProducerId = null;
        this.cameraProducerId = null;

        try {
            if (this.native()) {
                if (hadMic) {
                    this.micProducerId = await this.publishNative('mic');
                }

                if (hadCamera) {
                    this.cameraProducerId = await this.publishNative('camera');
                }
            } else {
                if (hadMic && this.micTrack) {
                    this.micProducerId = (await this.app.sfu.produce(this.micTrack, 'mic', Voice.MIC_OPTIONS)).id;
                }

                if (hadCamera && this.cameraTrack) {
                    this.cameraProducerId = (await this.app.sfu.produce(this.cameraTrack, 'camera')).id;
                }
            }

            if (this.muted || this.serverMuted) {
                await this.applyMute();
            }
        } catch (failure) {
            this.app.log('voice.republish.error', { message: failure.message ?? String(failure) });
            this.app.toast(`o microfone não voltou depois da reconexão: ${failure.message ?? failure}`, true);
        }

        this.paintBar();
    }
}
