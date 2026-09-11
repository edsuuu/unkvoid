import { SfuClient } from './sfu-client.js';

class Watcher {
    constructor(root, code) {
        this.root = root;
        this.code = code;
        this.sfu = null;
        this.tiles = new Map();
        this.consuming = new Set();
    }

    start() {
        this.root.querySelector('[data-join]').addEventListener('submit', (event) => {
            event.preventDefault();
            this.join(this.root.querySelector('[data-name]').value.trim());
        });
    }

    async join(name) {
        if (name === '') {
            return;
        }

        this.status('conectando…');
        this.sfu = new SfuClient();
        this.sfu.addEventListener('newProducer', (event) => this.consume(event.detail.producerId, event.detail.peerId));
        this.sfu.addEventListener('producerClosed', (event) => this.remove(event.detail.peerId));
        this.sfu.addEventListener('peerLeft', (event) => this.remove(event.detail.peerId));
        this.sfu.addEventListener('closed', () => this.status('a conexão caiu. Recarregue a página para entrar de novo.'));
        this.sfu.addEventListener('kicked', () => this.status('você foi removido desta sala.'));

        try {
            const joined = await this.sfu.connect(`${location.origin.replace(/^http/, 'ws')}/sfu`, { room: this.code, name });

            this.root.querySelector('[data-entry]').hidden = true;
            this.root.querySelector('[data-room]').hidden = false;
            this.status('');

            if (!this.sfu.canWatch()) {
                this.status('este navegador não tem WebRTC.');

                return;
            }

            for (const peer of joined.peers ?? []) {
                for (const producer of peer.producers ?? []) {
                    await this.consume(producer.producerId, peer.peerId);
                }
            }

            this.paintEmpty();
        } catch (failure) {
            this.status(`não deu para entrar: ${failure.message ?? failure}`);
        }
    }

    async consume(producerId, peerId) {
        if (this.consuming.has(producerId)) {
            return;
        }

        this.consuming.add(producerId);

        try {
            const { consumer } = await this.sfu.consume(producerId);
            const tile = this.tile(peerId);
            const media = tile.querySelector(consumer.kind === 'video' ? 'video' : 'audio');

            media.srcObject = new MediaStream([consumer.track]);
            await media.play().catch(() => this.status('clique na tela para liberar o som.'));
            this.paintEmpty();
        } catch (failure) {
            this.status(`não deu para receber a tela: ${failure.message ?? failure}`);
        } finally {
            this.consuming.delete(producerId);
        }
    }

    tile(peerId) {
        if (this.tiles.has(peerId)) {
            return this.tiles.get(peerId);
        }

        const template = this.root.querySelector('template');
        const tile = template.content.firstElementChild.cloneNode(true);

        tile.dataset.peer = peerId;
        tile.querySelector('[data-peer-name]').textContent = this.sfu.peers.get(peerId)?.name ?? 'alguém';
        tile.querySelector('[data-fullscreen]').addEventListener('click', () => tile.querySelector('video').requestFullscreen?.());
        this.root.querySelector('[data-stage]').appendChild(tile);
        this.tiles.set(peerId, tile);

        return tile;
    }

    remove(peerId) {
        this.tiles.get(peerId)?.remove();
        this.tiles.delete(peerId);
        this.paintEmpty();
    }

    paintEmpty() {
        this.root.querySelector('[data-empty]').hidden = this.tiles.size > 0;
    }

    status(text) {
        this.root.querySelector('[data-status]').textContent = text;
    }
}

const root = document.querySelector('[data-watch]');

if (root) {
    new Watcher(root, root.dataset.watch).start();
}
