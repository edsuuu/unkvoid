import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { Participant } from '../lib/Participant.mjs';
import { cleanVideo, decode, guest, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';

const SCREEN = { width: 1920, height: 1080, fps: 60, bitrate: 8_000_000 };
const CAMERA = { width: 640, height: 360, fps: 30, bitrate: 800_000 };
const STEADY_MS = 8000;

let sfu;
const people = [];

before(async () => {
    sfu = await new SfuProcess({ ...ports(0), workers: 2 }).start();
});

after(async () => {
    for (const person of people) {
        person.crash();
    }

    await sfu?.stop();
});

test('a. tela 1080p60, câmera 360p30 e microfone chegam em ordem, sem buraco, no tamanho e no fps pedidos', async () => {
    const room = 'e2e-transmitir';
    const ana = await new Participant({ name: 'ana', url: sfu.url, identity: guest(room, 'ana') }).join();
    const bia = await new Participant({ name: 'bia', url: sfu.url, identity: guest(room, 'bia'), keepStreams: true }).join();
    const caio = await new Participant({ name: 'caio', url: sfu.url, identity: guest(room, 'caio') }).join();

    people.push(ana, bia, caio);

    await ana.publish('screen', SCREEN);
    await ana.publish('camera', CAMERA);
    await ana.publish('mic');

    for (const watcher of [bia, caio]) {
        await waitFor(() => watcher.watchSummaries().filter(summary => summary.decodable > 0 || summary.packets > 0).length === 3, 5000, `${watcher.name} receiving three streams`);
    }

    // A janela de medida começa com tudo já chegando: o primeiro quadro tem cenário próprio (b).
    bia.resetWatches();
    caio.resetWatches();
    await sleep(STEADY_MS);

    const watched = [...bia.watchSummaries(), ...caio.watchSummaries()];
    const problems = [];

    for (const summary of watched) {
        if (summary.label.endsWith(':screen')) {
            problems.push(...cleanVideo(summary, SCREEN).map(problem => `${summary.label}: ${problem}`));
        } else if (summary.label.endsWith(':camera')) {
            problems.push(...cleanVideo(summary, CAMERA).map(problem => `${summary.label}: ${problem}`));
        } else {
            if (summary.lost > 0 || summary.outOfOrder > 0 || summary.clockErrors > 0 || summary.unknown > 0) {
                problems.push(`${summary.label}: som com ${summary.lost} perdidos, ${summary.outOfOrder} fora de ordem, ${summary.clockErrors} saltos de relógio`);
            }

            if (summary.maxGapMs > 500) {
                problems.push(`${summary.label}: som parado por ${summary.maxGapMs} ms`);
            }
        }

        if (summary.skipped > 0) {
            problems.push(`${summary.label}: ${summary.skipped} quadros pulados`);
        }
    }

    const screenStream = bia.watchSummaries().find(summary => summary.label.endsWith(':screen'));
    const decoded = decode(bia.watchOf(screenStream.producerId).annexB(), 'a-tela-bia');
    const totals = await sfu.totals();

    record('a', { screen: SCREEN, camera: CAMERA, steadyMs: STEADY_MS, watched, decoded, totals, sender: ana.sender.stats });

    assert.deepEqual(problems, []);
    assert.equal(decoded.errors, '', `o ffmpeg reclamou: ${decoded.errors}`);
    assert.equal(decoded.width, SCREEN.width);
    assert.equal(decoded.height, SCREEN.height);
    assert.ok(decoded.frames >= screenStream.decodable, `o ffmpeg decodificou ${decoded.frames} de ${screenStream.decodable}`);
    assert.deepEqual({ producers: totals.producers, consumers: totals.consumers }, { producers: 3, consumers: 6 });
});

test('a. tela e câmera juntas no ritmo do app: a câmera derruba o ritmo da tela e o quadro atrasa', async () => {
    const room = 'e2e-ritmo';
    const ana = await new Participant({ name: 'ana', url: sfu.url, identity: guest(room, 'ana'), watch: false, pacing: 'sum' }).join();
    const bia = await new Participant({ name: 'bia', url: sfu.url, identity: guest(room, 'bia') }).join();

    people.push(ana, bia);

    await ana.publish('screen', { width: 1920, height: 1080, fps: 30, bitrate: 6_000_000 });
    await ana.publish('camera', CAMERA);
    await waitFor(() => bia.watchSummaries().filter(summary => summary.decodable > 0).length === 2, 5000, 'bia watching');
    bia.resetWatches();
    await sleep(STEADY_MS);

    const screen = bia.watchSummaries().find(summary => summary.label.endsWith(':screen'));

    record('a-ritmo-do-app', { screen, droppedBySender: ana.sender.pacer.dropped });

    // Com o ritmo somado (o teste acima) a tela chega em ~20 ms no p99.
    assert.ok(screen.latencyP99Ms <= 100, `a tela chegou com ${screen.latencyP99Ms} ms no p99`);
    assert.ok(screen.maxFreezeMs <= 500, `a tela parou ${screen.maxFreezeMs} ms`);
});
