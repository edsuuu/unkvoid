import assert from 'node:assert/strict';
import { availableParallelism, cpus } from 'node:os';
import { after, test } from 'node:test';

import { cpuBetween, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';
import { ThreadedParticipant } from '../lib/ThreadedParticipant.mjs';

const SCREEN = { width: 1920, height: 1080, fps: 30, bitrate: 6_000_000 };
const CAMERA = { width: 640, height: 360, fps: 30, bitrate: 600_000 };
const PEOPLE = 10;
const SCREENS = 2;
const MEASURE_MS = Number(process.env.E2E_LOAD_MS ?? 30_000);

const sfus = [];
const people = [];

after(async () => {
    await Promise.all(people.map(person => person.crash()));

    for (const sfu of sfus) {
        await sfu.stop();
    }
});

/**
 * A sala cheia: todo mundo com câmera e microfone, duas telas, e todo mundo assistindo tudo
 * (o `consume_all` do app). Mede a CPU do SFU (o Node e cada worker, pelo `/stats`) numa
 * janela parada, e o que cada pessoa viu nela.
 */
const fullRoom = async (sfu, room) => {
    const crowd = [];

    for (let index = 0; index < PEOPLE; index += 1) {
        crowd.push(await new ThreadedParticipant({ name: `p${index}`, url: sfu.url, room, gate: 'immediate', receiverReports: true, extendedReports: true }).join());
    }

    people.push(...crowd);

    for (const [index, person] of crowd.entries()) {
        if (index < SCREENS) {
            await person.publish('screen', SCREEN);
        }

        await person.publish('camera', CAMERA);
        await person.publish('mic');
    }

    // As câmeras dos outros e as telas dos outros: quem compartilha não assiste a própria.
    const videosOf = index => PEOPLE - 1 + (index < SCREENS ? SCREENS - 1 : SCREENS);

    await waitFor(
        async () => (await Promise.all(crowd.map(person => person.watchSummaries()))).every((summaries, index) => summaries.filter(summary => summary.decodable > 0).length === videosOf(index)),
        20_000,
        'everyone watching everything',
        500,
    );
    await sleep(3000);
    await Promise.all(crowd.map(person => person.resetWatches()));

    const harnessBefore = process.cpuUsage();
    const before = await sfu.cpu();

    await sleep(MEASURE_MS);

    const afterSample = await sfu.cpu();
    const harnessAfter = process.cpuUsage(harnessBefore);
    const watched = (await Promise.all(crowd.map(person => person.watchSummaries()))).flat();
    const videos = watched.filter(summary => summary.decodable !== undefined);
    const audios = watched.filter(summary => summary.decodable === undefined);
    const holes = (await Promise.all(crowd.map(person => person.holes()))).flat();
    const datagrams = (await Promise.all(crowd.map(person => person.receiverStats()))).reduce((total, stats) => total + (stats?.datagrams ?? 0), 0);

    return {
        cpu: cpuBetween(before, afterSample),
        harnessCpuPercent: Math.round(((harnessAfter.user + harnessAfter.system) / 1000 / MEASURE_MS) * 1000) / 10,
        memory: await sfu.memory(),
        totals: await sfu.totals(),
        consumers: afterSample.stats.workers.reduce((total, worker) => total + worker.consumers, 0),
        datagramsPerSecondOut: Math.round(datagrams / ((MEASURE_MS + 3000) / 1000)),
        videos: {
            streams: videos.length,
            withFreezeOver500: videos.filter(summary => summary.maxFreezeMs > 500).map(summary => `${summary.label}: ${summary.maxFreezeMs} ms`),
            worstFreezeMs: Math.max(...videos.map(summary => summary.maxFreezeMs)),
            outOfOrder: videos.reduce((total, summary) => total + summary.outOfOrder, 0),
            screenFps: videos.filter(summary => summary.label.endsWith(':screen')).map(summary => summary.fps),
            cameraFpsMin: Math.min(...videos.filter(summary => summary.label.endsWith(':camera')).map(summary => summary.fps)),
            latencyP99Ms: Math.max(...videos.map(summary => summary.latencyP99Ms ?? 0)),
        },
        holes,
        audios: { streams: audios.length, lost: audios.reduce((total, summary) => total + summary.lost, 0), worstGapMs: Math.max(...audios.map(summary => summary.maxGapMs)) },
        machine: { cores: availableParallelism(), model: cpus()[0]?.model },
    };
};

test(`h. carga: ${PEOPLE} pessoas, ${SCREENS} telas 1080p30 e ${PEOPLE} câmeras 360p30, todo mundo assistindo tudo, num router só (como na VPS)`, async () => {
    const sfu = await new SfuProcess({ ...ports(9), workers: 3 }).start();

    sfus.push(sfu);

    const result = await fullRoom(sfu, 'e2e-carga');

    record('h-um-router', result);

    await Promise.all(people.splice(0).map(person => person.crash()));
    await sfu.stop();

    assert.equal(result.videos.streams, PEOPLE * (PEOPLE - 1) + SCREENS * (PEOPLE - 1));
    assert.equal(result.videos.outOfOrder, 0);
    assert.deepEqual(result.videos.withFreezeOver500, []);
    assert.equal(result.audios.lost, 0);
});

test(`h. a mesma carga espalhada em dois workers (SFU_PEERS_PER_ROUTER=5)`, async () => {
    const sfu = await new SfuProcess({ ...ports(10), workers: 3, peersPerRouter: 5 }).start();

    sfus.push(sfu);

    const result = await fullRoom(sfu, 'e2e-carga-espalhada');

    record('h-dois-routers', result);

    assert.equal(result.videos.outOfOrder, 0);
    assert.deepEqual(result.videos.withFreezeOver500, []);
    assert.equal(result.audios.lost, 0);
});
