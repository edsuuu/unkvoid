/** O lado de dentro do `ThreadedParticipant`: um `Participant` comum, guiado por mensagens. */
import { parentPort, workerData } from 'node:worker_threads';

import { Participant } from './Participant.mjs';

const { name, url, room, ...options } = workerData;
const person = new Participant({ name, url, identity: async () => ({ room, name, installId: name }), ...options });

const methods = {
    join: () => person.join().then(() => null),
    publish: (source, settings) => person.publish(source, settings).then(() => null),
    watchSummaries: () => person.watchSummaries(),
    resetWatches: () => person.resetWatches(),
    holes: () =>
        [...(person.receiver?.routes.values() ?? [])]
            .filter(route => route.video && route.recovery.counters.lost > 0)
            .map(route => `${route.watch.label}: ${route.recovery.counters.lost}`),
    receiverStats: () => person.receiver?.stats ?? null,
    crash: () => person.crash(),
};

parentPort.on('message', async ({ id, method, args }) => {
    try {
        parentPort.postMessage({ id, result: (await methods[method](...args)) ?? null });
    } catch (error) {
        parentPort.postMessage({ id, error: error.message });
    }
});
