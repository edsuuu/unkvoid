import { Worker } from 'node:worker_threads';

/**
 * Um `Participant` numa thread própria. Na carga, dez pessoas recebendo 17 Mb/s cada passam de
 * um núcleo, e uma thread só do harness atrasando derruba pacote no socket — que viraria
 * defeito do SFU no relatório. Cada pessoa num núcleo, o harness para de ser o gargalo.
 *
 * Os métodos chamam o mesmo nome no `Participant` do outro lado e devolvem o resultado.
 */
export class ThreadedParticipant {
    constructor(options) {
        this.name = options.name;
        this.nextId = 1;
        this.pending = new Map();
        this.worker = new Worker(new URL('./participant-thread.mjs', import.meta.url), { workerData: options });
        this.worker.on('message', ({ id, result, error }) => {
            const waiter = this.pending.get(id);

            this.pending.delete(id);

            if (error) {
                waiter?.reject(new Error(error));
            } else {
                waiter?.resolve(result);
            }
        });
        this.worker.on('error', error => {
            for (const waiter of this.pending.values()) {
                waiter.reject(error);
            }

            this.pending.clear();
        });
    }

    call(method, ...args) {
        const id = this.nextId++;

        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.worker.postMessage({ id, method, args });
        });
    }

    join() {
        return this.call('join').then(() => this);
    }

    publish(source, options) {
        return this.call('publish', source, options);
    }

    watchSummaries() {
        return this.call('watchSummaries');
    }

    resetWatches() {
        return this.call('resetWatches');
    }

    holes() {
        return this.call('holes');
    }

    receiverStats() {
        return this.call('receiverStats');
    }

    async crash() {
        await this.call('crash').catch(() => {});
        await this.worker.terminate();
    }
}
