import assert from 'node:assert/strict';
import { after, test } from 'node:test';

import { Participant } from '../lib/Participant.mjs';
import { cleanVideo, guest, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';

const SCREEN = { width: 1280, height: 720, fps: 30, bitrate: 3_000_000 };
const CAMERA = { width: 640, height: 360, fps: 30, bitrate: 800_000 };

const sfus = [];
const people = [];

after(async () => {
    for (const person of people) {
        person.crash();
    }

    for (const sfu of sfus) {
        await sfu.stop();
    }
});

const join = async (sfu, room, name, options = {}) => {
    const person = await new Participant({ name, url: sfu.url, identity: guest(room, name), gate: 'immediate', ...options }).join();

    people.push(person);

    return person;
};

const seesVideo = (watcher, from, source) => {
    const summary = watcher.watchSummaries().find(candidate => candidate.label === `${watcher.name}<-${from}:${source}`);

    return summary && summary.decodable > 0 ? summary : null;
};

/** O worker que tem mais routers agora: é onde a sala que acabou de nascer caiu. */
const busiest = stats => stats.workers.reduce((best, worker) => (worker.routers > best.routers ? worker : best));

test('f. o worker que morre leva só a sala dele: quem estava nela volta a ver sozinho, a outra sala nem percebe', async () => {
    const sfu = await new SfuProcess({ ...ports(5), workers: 3 }).start();

    sfus.push(sfu);

    const ana = await join(sfu, 'e2e-worker-um', 'ana', { watch: false });
    const bia = await join(sfu, 'e2e-worker-um', 'bia');

    await ana.publish('screen', SCREEN);
    await ana.publish('mic');

    const victim = busiest(await sfu.stats());
    const caio = await join(sfu, 'e2e-worker-dois', 'caio', { watch: false });
    const davi = await join(sfu, 'e2e-worker-dois', 'davi');

    await caio.publish('screen', SCREEN);
    await caio.publish('mic');
    await waitFor(() => seesVideo(bia, 'ana', 'screen') && seesVideo(davi, 'caio', 'screen'), 5000, 'both rooms watching');

    const placement = await sfu.stats();

    assert.equal(placement.workers.filter(worker => worker.routers > 0).length, 2, 'as duas salas caíram no mesmo worker');

    davi.resetWatches();

    const killedAt = Date.now();

    await sfu.killWorker(victim.index);

    const back = await waitFor(
        () => {
            const route = [...bia.receiver?.routes.values() ?? []].find(candidate => candidate.watch.label === 'bia<-ana:screen' && candidate.watch.firstDecodableAt > killedAt);

            return route ? route.watch.firstDecodableAt - killedAt : null;
        },
        15_000,
        'bia watching ana again',
    );

    await sleep(2000);

    const revived = await waitFor(async () => {
        const stats = await sfu.stats();
        const worker = stats.workers[victim.index];

        return !worker.closed && worker.pid !== victim.pid ? stats : null;
    }, 15_000, 'worker revived');
    const untouched = davi.watchSummaries().find(summary => summary.label === 'davi<-caio:screen');
    const health = await sfu.health();
    const totals = await sfu.totals();

    record('f-worker', {
        backToWatchingMs: back,
        closes: { ana: ana.events('closed').map(entry => entry.data.code), bia: bia.events('closed').map(entry => entry.data.code), caio: caio.events('closed').length, davi: davi.events('closed').length },
        joins: { ana: ana.joins, bia: bia.joins },
        untouched,
        health,
        totals,
        revivedPid: revived.workers[victim.index].pid,
    });

    assert.deepEqual(ana.events('closed').map(entry => entry.data.code), [1012]);
    assert.deepEqual(bia.events('closed').map(entry => entry.data.code), [1012]);
    assert.equal(ana.joins.fresh, 2, 'quem transmitia não entrou de novo');
    assert.ok(back <= 5000, `quem assistia voltou a ver em ${back} ms`);
    assert.equal(caio.events('closed').length + davi.events('closed').length, 0, 'a outra sala caiu junto');
    assert.deepEqual(cleanVideo(untouched, SCREEN), []);
    assert.equal(health.workersDown, 0);
    // Só o que está no ar: 2 producers por sala, 2 consumers por sala.
    assert.deepEqual({ producers: totals.producers, consumers: totals.consumers }, { producers: 4, consumers: 4 });
});

test('f. a sala espalhada em dois workers (pipeToRouter) publica e assiste, e o worker que morre leva só os dele', async () => {
    const sfu = await new SfuProcess({ ...ports(6), workers: 2, peersPerRouter: 2 }).start();

    sfus.push(sfu);

    const room = 'e2e-espalhada';
    const ana = await join(sfu, room, 'ana');
    const bia = await join(sfu, room, 'bia');

    await ana.publish('screen', SCREEN);
    await bia.publish('mic');

    const first = busiest(await sfu.stats());
    const caio = await join(sfu, room, 'caio');
    const davi = await join(sfu, room, 'davi');

    await caio.publish('camera', CAMERA);
    await davi.publish('mic');

    const spread = await sfu.stats();
    const everyone = [ana, bia, caio, davi];
    const watchingAll = () => [bia, caio, davi].every(person => seesVideo(person, 'ana', 'screen')) && [ana, bia, davi].every(person => seesVideo(person, 'caio', 'camera'));

    await waitFor(watchingAll, 5000, 'everyone watching across workers');

    for (const person of everyone) {
        person.resetWatches();
    }

    await sleep(3000);

    const crossed = everyone.flatMap(person => person.watchSummaries().filter(summary => summary.decodable !== undefined));
    const problemsBefore = crossed.flatMap(summary => cleanVideo(summary, summary.label.endsWith(':screen') ? SCREEN : CAMERA).map(problem => `${summary.label}: ${problem}`));
    // O worker da segunda metade: quem está nele cai, a primeira metade fica.
    const second = spread.workers.find(worker => worker.index !== first.index);

    bia.resetWatches();
    await sfu.killWorker(second.index);
    await waitFor(watchingAll, 15_000, 'everyone watching again after the worker died');
    await sleep(1000);

    const kept = bia.watchSummaries().find(summary => summary.label === 'bia<-ana:screen');
    const totals = await sfu.totals();

    record('f-espalhada', {
        spread: spread.workers.map(worker => ({ routers: worker.routers, transports: worker.transports, producers: worker.producers, consumers: worker.consumers })),
        problemsBefore,
        closes: Object.fromEntries(everyone.map(person => [person.name, person.events('closed').map(entry => entry.data.code)])),
        kept,
        totals,
    });

    assert.equal(spread.workers.filter(worker => worker.routers > 0).length, 2, 'a sala não se espalhou');
    assert.deepEqual(problemsBefore, []);
    assert.deepEqual([ana, bia].map(person => person.events('closed').length), [0, 0], 'quem estava no worker vivo caiu');
    assert.deepEqual([caio, davi].map(person => person.events('closed').map(entry => entry.data.code)), [[1012], [1012]]);
    assert.deepEqual(cleanVideo(kept, SCREEN), []);
});

test('f. a sala que encolhe devolve o router do outro worker: sem pipe mandando mídia para um router vazio', async () => {
    const sfu = await new SfuProcess({ ...ports(8), workers: 2, peersPerRouter: 2 }).start();

    sfus.push(sfu);

    const room = 'e2e-encolhe';
    const ana = await join(sfu, room, 'ana', { watch: false });
    const bia = await join(sfu, room, 'bia');

    await ana.publish('screen', SCREEN);
    await ana.publish('mic');
    await waitFor(() => seesVideo(bia, 'ana', 'screen'), 5000, 'bia watching');

    const alone = await sfu.totals();
    const caio = await join(sfu, room, 'caio');
    const davi = await join(sfu, room, 'davi');

    await waitFor(() => seesVideo(caio, 'ana', 'screen') && seesVideo(davi, 'ana', 'screen'), 5000, 'the second router watching through the pipe');

    const spread = await sfu.stats();

    await caio.leave();
    await davi.leave();
    await sleep(500);

    const shrunk = await sfu.stats();
    const totals = await sfu.totals();

    bia.resetWatches();
    await sleep(2000);

    const still = bia.watchSummaries().find(summary => summary.label === 'bia<-ana:screen');

    record('f-encolhe', {
        alone,
        spread: spread.workers.map(worker => ({ routers: worker.routers, transports: worker.transports, producers: worker.producers, consumers: worker.consumers })),
        shrunk: shrunk.workers.map(worker => ({ routers: worker.routers, transports: worker.transports, producers: worker.producers, consumers: worker.consumers })),
    });

    assert.equal(spread.workers.filter(worker => worker.routers > 0).length, 2, 'a sala não se espalhou');
    assert.deepEqual(
        { routers: totals.routers, transports: totals.transports, producers: totals.producers, consumers: totals.consumers },
        { routers: alone.routers, transports: alone.transports, producers: alone.producers, consumers: alone.consumers },
    );
    assert.deepEqual(cleanVideo(still, SCREEN), []);
});
