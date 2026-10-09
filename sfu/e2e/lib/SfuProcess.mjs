import { spawn } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const SECRET = process.env.SFU_SECRET ?? 'segredo-do-harness-e2e-com-mais-de-32-caracteres';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/**
 * Um SFU de verdade (`dist/server.js`) num processo próprio, com a configuração que o
 * cenário pede, e o que o harness precisa ver dele: o `/health`, o `/stats` assinado (o que
 * o mediasoup segura, worker por worker) e a CPU do Node e de cada worker.
 */
export class SfuProcess {
    constructor({ port, workers = 2, plainPort, plainPorts = 64, mediaPort, peersPerRouter = 10, env = {} }) {
        this.port = port;
        this.workers = workers;
        this.env = {
            SFU_SECRET: SECRET,
            SFU_HOST: '127.0.0.1',
            SFU_PORT: String(port),
            SFU_WORKERS: String(workers),
            SFU_MEDIA_PORT: String(mediaPort ?? port + 37000),
            SFU_PLAIN_PORT: String(plainPort ?? port + 38000),
            SFU_PLAIN_PORTS: String(plainPorts),
            SFU_PEERS_PER_ROUTER: String(peersPerRouter),
            SFU_CONNECTIONS_PER_MINUTE: '100000',
            SFU_ANNOUNCED_ADDRESS: '127.0.0.1',
            SFU_LARAVEL_URL: '',
            ...env,
        };
        this.url = `ws://127.0.0.1:${port}/sfu`;
        this.http = `http://127.0.0.1:${port}`;
        this.output = [];
    }

    async start() {
        this.process = spawn(process.execPath, ['dist/server.js'], { cwd: ROOT, env: { ...process.env, ...this.env }, stdio: ['ignore', 'pipe', 'pipe'] });
        this.process.stdout.on('data', chunk => this.output.push(chunk.toString()));
        this.process.stderr.on('data', chunk => this.output.push(chunk.toString()));
        this.exited = new Promise(resolve => this.process.once('exit', resolve));

        const deadline = Date.now() + 15_000;

        while (Date.now() < deadline) {
            if (this.process.exitCode !== null) {
                throw new Error(`the SFU exited on boot:\n${this.log()}`);
            }

            const health = await this.health().catch(() => null);

            // Outro SFU na mesma porta também responde o `/health`: só vale se este está de pé.
            if (health?.ok) {
                await new Promise(resolve => setTimeout(resolve, 300));

                if (this.process.exitCode !== null) {
                    throw new Error(`the SFU exited on boot (port ${this.port} taken?):\n${this.log()}`);
                }

                return this;
            }

            await new Promise(resolve => setTimeout(resolve, 100));
        }

        throw new Error(`the SFU did not answer /health in 15 s:\n${this.log()}`);
    }

    log() {
        return this.output.join('');
    }

    async health() {
        const response = await fetch(`${this.http}/health`, { signal: AbortSignal.timeout(2000) });

        return response.json();
    }

    signed(method, path, body = '') {
        const at = String(Math.floor(Date.now() / 1000));
        const signature = createHmac('sha256', SECRET).update(`${at}\n${method}\n${path}\n${body}`).digest('hex');

        return fetch(`${this.http}${path}`, {
            method,
            ...(body === '' ? {} : { body }),
            headers: { 'content-type': 'application/json', 'x-unkvoid-timestamp': at, 'x-unkvoid-signature': signature },
            signal: AbortSignal.timeout(5000),
        });
    }

    async stats() {
        const response = await this.signed('GET', '/stats');

        if (!response.ok) {
            throw new Error(`/stats answered ${response.status}`);
        }

        return response.json();
    }

    async presence() {
        return (await this.signed('GET', '/presence')).json();
    }

    /** O que o mediasoup segura somado em todos os workers. */
    async totals() {
        const stats = await this.stats();

        return stats.workers.reduce(
            (total, worker) => ({
                ...total,
                routers: total.routers + worker.routers,
                transports: total.transports + worker.transports,
                producers: total.producers + worker.producers,
                consumers: total.consumers + worker.consumers,
            }),
            { rooms: stats.rooms, peers: stats.peers, routers: 0, transports: 0, producers: 0, consumers: 0 },
        );
    }

    /** A CPU gasta até agora, em milissegundos: o Node e cada worker, pelo `/stats`. */
    async cpu() {
        const stats = await this.stats();

        return {
            at: Date.now(),
            node: stats.node.cpuMs,
            workers: stats.workers.map(worker => worker.cpuMs),
            rssKb: stats.node.rssKb + stats.workers.reduce((total, worker) => total + worker.maxRssKb, 0),
            stats,
        };
    }

    /** A memória residente agora (não o pico) do Node e de cada worker, em KB, pelo `/proc`. */
    async memory() {
        const stats = await this.stats();
        const rss = pid => {
            try {
                return Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${pid}/status`, 'utf8'))?.[1] ?? 0);
            } catch {
                return 0;
            }
        };
        const workers = stats.workers.map(worker => rss(worker.pid));

        return { node: rss(this.process.pid), workers, totalKb: rss(this.process.pid) + workers.reduce((sum, value) => sum + value, 0), heapUsedKb: stats.node.heapUsedKb };
    }

    async killWorker(index) {
        const stats = await this.stats();
        const pid = stats.workers[index].pid;

        process.kill(pid, 'SIGKILL');

        return pid;
    }

    async stop() {
        if (!this.process || this.process.exitCode !== null) {
            return;
        }

        this.process.kill('SIGTERM');
        await Promise.race([this.exited, new Promise(resolve => setTimeout(resolve, 5000))]);

        if (this.process.exitCode === null) {
            this.process.kill('SIGKILL');
        }
    }
}
