import { createConnection, createServer } from 'node:net';

/**
 * O TCP da sinalização com o cabo puxado: no `cut` nada mais passa em sentido nenhum, mas
 * a conexão não fecha — é o socket meio aberto que nunca manda FIN nem RST. No `restore` o
 * que estava parado volta a andar, como o TCP de verdade depois da rede voltar. Conexão
 * nova durante o corte espera a rede.
 */
export class TcpProxy {
    constructor({ host, port }) {
        this.host = host;
        this.port = port;
        this.down = false;
        this.pairs = new Set();
        this.waiting = [];
    }

    async start() {
        this.server = createServer(client => {
            if (this.down) {
                client.pause();
                this.waiting.push(client);

                return;
            }

            this.connect(client);
        });

        await new Promise(resolve => this.server.listen(0, '127.0.0.1', resolve));
        this.localPort = this.server.address().port;

        return this;
    }

    connect(client) {
        const upstream = createConnection({ host: this.host, port: this.port });
        const pair = { client, upstream };

        this.pairs.add(pair);

        // O FIN de um lado só chega ao outro quando a rede volta.
        const end = () => {
            if (this.down) {
                pair.closing = true;

                return;
            }

            this.pairs.delete(pair);
            client.destroy();
            upstream.destroy();
        };

        client.on('data', chunk => (this.down ? (pair.heldUp ??= []).push(chunk) : upstream.write(chunk)));
        upstream.on('data', chunk => (this.down ? (pair.heldDown ??= []).push(chunk) : client.write(chunk)));
        client.on('close', end);
        upstream.on('close', end);
        client.on('error', end);
        upstream.on('error', end);
        client.resume();
    }

    url(path = '/sfu') {
        return `ws://127.0.0.1:${this.localPort}${path}`;
    }

    cut() {
        this.down = true;
    }

    restore() {
        this.down = false;

        for (const pair of this.pairs) {
            for (const chunk of pair.heldUp ?? []) {
                pair.upstream.write(chunk);
            }

            for (const chunk of pair.heldDown ?? []) {
                pair.client.write(chunk);
            }

            pair.heldUp = [];
            pair.heldDown = [];

            if (pair.closing) {
                this.pairs.delete(pair);
                pair.client.destroy();
                pair.upstream.destroy();
            }
        }

        for (const client of this.waiting.splice(0)) {
            if (!client.destroyed) {
                this.connect(client);
            }
        }
    }

    async close() {
        for (const { client, upstream } of this.pairs) {
            client.destroy();
            upstream.destroy();
        }

        await new Promise(resolve => this.server.close(resolve));
    }
}
