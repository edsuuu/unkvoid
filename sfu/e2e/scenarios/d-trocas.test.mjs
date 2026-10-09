import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { Participant } from '../lib/Participant.mjs';
import { cleanVideo, decode, guest, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';

const HD = { width: 1280, height: 720, fps: 30, bitrate: 4_000_000 };
const FULL_HD = { width: 1920, height: 1080, fps: 30, bitrate: 6_000_000 };

let sfu;
const people = [];

before(async () => {
    sfu = await new SfuProcess({ ...ports(3), workers: 2 }).start();
});

after(async () => {
    for (const person of people) {
        person.crash();
    }

    await sfu?.stop();
});

const join = async (room, name, options = {}) => {
    const person = await new Participant({ name, url: sfu.url, identity: guest(room, name), ...options }).join();

    people.push(person);

    return person;
};

const screenRoute = (watcher, from) => [...watcher.receiver?.routes.values() ?? []].find(route => route.watch.label === `${watcher.name}<-${from}:screen`);

const screenOf = (watcher, from) => watcher.watchSummaries().find(summary => summary.label === `${watcher.name}<-${from}:screen`);

const firstFrameOf = async (watcher, from, since, timeoutMs = 3000) =>
    waitFor(
        () => {
            const route = [...watcher.receiver?.routes.values() ?? []].find(candidate => candidate.watch.label === `${watcher.name}<-${from}:screen`);

            return route?.watch.firstDecodableAt ? route.watch.firstDecodableAt - since : null;
        },
        timeoutMs,
        `${watcher.name} seeing ${from}'s screen`,
    );

test('d. trocar a resolução no meio (720p → 1080p): o IDR novo chega sem parar a imagem', async () => {
    const room = 'e2e-resolucao';
    const ana = await join(room, 'ana', { watch: false });
    const bia = await join(room, 'bia', { keepStreams: true });

    await ana.publish('screen', HD);
    await firstFrameOf(bia, 'ana', Date.now());
    await sleep(2000);
    bia.resetWatches();
    ana.resize('screen', FULL_HD.width, FULL_HD.height);
    ana.tracks.get('screen').bitrate = FULL_HD.bitrate;
    await sleep(3000);

    const summary = screenOf(bia, 'ana');
    const decoded = decode(bia.watchOf(summary.producerId).annexB(), 'd-resolucao');

    record('d-resolucao', { summary, decoded });

    assert.deepEqual(summary.resolutions, ['1280x720', '1920x1080']);
    assert.deepEqual(cleanVideo(summary, FULL_HD), []);
    assert.equal(decoded.errors, '');
    assert.deepEqual([decoded.width, decoded.lastWidth, decoded.lastHeight], [HD.width, FULL_HD.width, FULL_HD.height]);
});

test('d. trocar de tela (parar e compartilhar outra): quem assiste sai da velha e vê a nova em até 1 s', async () => {
    const room = 'e2e-trocar-tela';
    const ana = await join(room, 'ana', { watch: false, gate: 'immediate' });
    const bia = await join(room, 'bia');

    await ana.publish('mic');
    await ana.publish('screen', FULL_HD);
    await firstFrameOf(bia, 'ana', Date.now());

    const old = screenOf(bia, 'ana').producerId;

    await ana.unpublish('screen');
    await waitFor(() => !bia.consumers.has(old), 2000, 'old screen closed for bia');

    const startedAt = Date.now();

    await ana.publish('screen', HD);

    const firstFrameMs = await firstFrameOf(bia, 'ana', startedAt);

    await sleep(2000);

    const summary = screenOf(bia, 'ana');
    const totals = await sfu.totals();

    record('d-trocar-tela', { firstFrameMs, summary, totals });

    assert.notEqual(summary.producerId, old);
    assert.ok(firstFrameMs <= 1000, `primeiro quadro da tela nova em ${firstFrameMs} ms`);
    assert.deepEqual(cleanVideo(summary, HD), []);
});

test('d. parar tudo e recomeçar: o servidor solta o transporte de quem parou e o novo funciona', async () => {
    const room = 'e2e-parar';
    const ana = await join(room, 'ana', { watch: false, gate: 'immediate' });
    const bia = await join(room, 'bia');

    await ana.publish('screen', HD);
    await ana.publish('mic');
    await firstFrameOf(bia, 'ana', Date.now());

    const playing = await sfu.totals();

    await ana.unpublish('screen');
    await ana.unpublish('mic');
    await waitFor(async () => (await sfu.totals()).producers === playing.producers - 2, 2000, 'producers released');

    const stopped = await sfu.totals();

    await sleep(1000);

    const startedAt = Date.now();

    await ana.publish('screen', HD);
    await ana.publish('mic');

    const firstFrameMs = await firstFrameOf(bia, 'ana', startedAt);

    await sleep(2000);

    const summary = screenOf(bia, 'ana');
    const restarted = await sfu.totals();

    record('d-parar', { playing, stopped, restarted, firstFrameMs, summary });

    // Parado: sem producer, sem consumer, e o transporte de subida fechado (o de chegada da Bia fica).
    assert.deepEqual({ producers: stopped.producers, consumers: stopped.consumers, transports: stopped.transports }, { producers: playing.producers - 2, consumers: playing.consumers - 2, transports: playing.transports - 1 });
    assert.deepEqual({ producers: restarted.producers, consumers: restarted.consumers, transports: restarted.transports }, { producers: playing.producers, consumers: playing.consumers, transports: playing.transports });
    assert.ok(firstFrameMs <= 1000, `primeiro quadro depois de recomeçar em ${firstFrameMs} ms`);
    assert.deepEqual(cleanVideo(summary, HD), []);
});

test('d. duas telas ao mesmo tempo na mesma sala: quem assiste vê as duas, sem uma atrapalhar a outra', async () => {
    const room = 'e2e-duas-telas';
    const ana = await join(room, 'ana');
    const caio = await join(room, 'caio');
    const bia = await join(room, 'bia');

    await ana.publish('screen', FULL_HD);
    await ana.publish('mic');
    await caio.publish('screen', HD);
    await caio.publish('mic');
    await firstFrameOf(bia, 'ana', Date.now());
    await firstFrameOf(bia, 'caio', Date.now());
    bia.resetWatches();
    ana.resetWatches();
    caio.resetWatches();
    await sleep(5000);

    const seen = { bia: bia.watchSummaries(), ana: ana.watchSummaries(), caio: caio.watchSummaries() };
    const problems = [
        ...cleanVideo(screenOf(bia, 'ana'), FULL_HD).map(problem => `bia<-ana: ${problem}`),
        ...cleanVideo(screenOf(bia, 'caio'), HD).map(problem => `bia<-caio: ${problem}`),
        ...cleanVideo(screenOf(ana, 'caio'), HD).map(problem => `ana<-caio: ${problem}`),
        ...cleanVideo(screenOf(caio, 'ana'), FULL_HD).map(problem => `caio<-ana: ${problem}`),
    ];

    record('d-duas-telas', seen);

    assert.deepEqual(problems, []);
});

test('d. várias pessoas assistindo a mesma tela: todas recebem tudo, na ordem', async () => {
    const room = 'e2e-plateia';
    const ana = await join(room, 'ana', { watch: false });
    const watchers = [];

    await ana.publish('screen', FULL_HD);
    await ana.publish('mic');

    for (let index = 0; index < 8; index += 1) {
        watchers.push(await join(room, `v${index}`));
    }

    for (const watcher of watchers) {
        await firstFrameOf(watcher, 'ana', Date.now(), 5000);
        watcher.resetWatches();
    }

    await sleep(5000);

    const seen = watchers.map(watcher => screenOf(watcher, 'ana'));
    const problems = seen.flatMap(summary => cleanVideo(summary, FULL_HD).map(problem => `${summary.label}: ${problem}`));
    const watchersEvent = ana.events('watchers').at(-1)?.data;

    record('d-plateia', { seen, watchers: watchersEvent });

    assert.deepEqual(problems, []);
    assert.equal(watchersEvent?.watchers.length, 8, 'quem transmite não vê as 8 pessoas na plateia');
});

test('d. quem assiste pausa e retoma (janela minimizada): a imagem volta em até 1 s, com um quadro-chave só', async () => {
    const room = 'e2e-pausa';
    const ana = await join(room, 'ana', { watch: false, gate: 'immediate' });
    const bia = await join(room, 'bia');

    await ana.publish('screen', { ...FULL_HD, gopMs: 600_000 });
    await firstFrameOf(bia, 'ana', Date.now());

    const consumer = bia.consumers.get(screenOf(bia, 'ana').producerId);
    const track = ana.tracks.get('screen');

    await bia.client.call('pauseConsumer', { consumerId: consumer.consumerId });
    await sleep(2000);

    const keyframesBefore = track.keyframesSent;
    const resumedAt = Date.now();

    await bia.client.call('resumeConsumer', { consumerId: consumer.consumerId });

    const back = await waitFor(() => {
        const route = screenRoute(bia, 'ana');

        return route.watch.lastDecodableAt > resumedAt ? route.watch.lastDecodableAt - resumedAt : null;
    }, 3000, 'bia watching again after resume');

    await sleep(2000);

    record('d-pausa', { backMs: back, keyframes: track.keyframesSent - keyframesBefore });

    assert.ok(back <= 1000, `a imagem voltou ${back} ms depois do resume`);
    assert.equal(track.keyframesSent - keyframesBefore, 1);
});

test('d. quem transmite pausa e retoma a tela: a sala é avisada e a imagem volta em até 1 s', async () => {
    const room = 'e2e-pausa-tela';
    const ana = await join(room, 'ana', { watch: false, gate: 'immediate' });
    const bia = await join(room, 'bia');

    await ana.publish('screen', { ...FULL_HD, gopMs: 600_000 });
    await firstFrameOf(bia, 'ana', Date.now());

    const producerId = ana.producers.get('screen');

    await ana.client.call('pauseProducer', { producerId });
    await waitFor(() => bia.events('producerPaused').length > 0, 2000, 'bia told about the pause');
    await sleep(1500);

    const resumedAt = Date.now();

    await ana.client.call('resumeProducer', { producerId });
    await waitFor(() => bia.events('producerResumed').length > 0, 2000, 'bia told about the resume');

    const back = await waitFor(() => {
        const route = screenRoute(bia, 'ana');

        return route.watch.lastDecodableAt > resumedAt ? route.watch.lastDecodableAt - resumedAt : null;
    }, 3000, 'bia watching again after the producer resumed');

    record('d-pausa-tela', { backMs: back });

    assert.ok(back <= 1000, `a imagem voltou ${back} ms depois do resumeProducer`);
});
