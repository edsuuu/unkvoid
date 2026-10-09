import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { Participant } from '../lib/Participant.mjs';
import { cleanVideo, guest, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';
import { TcpProxy } from '../lib/TcpProxy.mjs';
import { UdpProxy } from '../lib/UdpProxy.mjs';

const SCREEN = { width: 1280, height: 720, fps: 30, bitrate: 3_000_000 };
const SMALL = { width: 320, height: 180, fps: 15, bitrate: 200_000 };
const OUTAGE_MS = 10_000;
const CYCLES = Number(process.env.E2E_CYCLES ?? 200);

let sfu;
const people = [];
const closers = [];

before(async () => {
    sfu = await new SfuProcess({ ...ports(7), workers: 2 }).start();
});

after(async () => {
    for (const person of people) {
        person.crash();
    }

    for (const close of closers) {
        await close();
    }

    await sfu?.stop();
});

/** Uma pessoa atrás de um cabo que se pode puxar: o WebSocket e o UDP passam pelos proxies. */
const unplugged = async (room, name, options = {}) => {
    const tcp = await new TcpProxy({ host: '127.0.0.1', port: sfu.port }).start();
    const udp = new UdpProxy();

    closers.push(() => tcp.close(), () => udp.close());

    const person = await new Participant({ name, url: tcp.url(), identity: guest(room, name), gate: 'immediate', relay: (host, port) => udp.relay(host, port), ...options }).join();

    people.push(person);

    return {
        person,
        cut: () => {
            tcp.cut();
            udp.cut();
        },
        restore: () => {
            tcp.restore();
            udp.restore();
        },
    };
};

const screenRoute = (watcher, from) => [...watcher.receiver?.routes.values() ?? []].find(route => route.watch.label === `${watcher.name}<-${from}:screen`);

/** Quanto depois de `since` a tela de `from` voltou a ter quadro novo para `watcher`. */
const backAfter = (watcher, from, since, timeoutMs) =>
    waitFor(
        () => {
            const route = screenRoute(watcher, from);

            return route?.watch.lastDecodableAt > since ? route.watch.lastDecodableAt - since : null;
        },
        timeoutMs,
        `${watcher.name} watching ${from} again`,
    );

const settled = async (expected, what) => waitFor(async () => {
    const totals = await sfu.totals();

    return totals.transports === expected.transports && totals.producers === expected.producers && totals.consumers === expected.consumers ? totals : null;
}, 40_000, what).catch(async () => sfu.totals());

test('g. quem assiste perde a rede por 10 s: volta a ver sem reiniciar o app, e o servidor não guarda nada a mais', async () => {
    const room = 'e2e-rede-assiste';
    const ana = await new Participant({ name: 'ana', url: sfu.url, identity: guest(room, 'ana'), gate: 'immediate' }).join();

    people.push(ana);

    const bia = await unplugged(room, 'bia');

    await ana.publish('screen', SCREEN);
    await ana.publish('mic');
    await backAfter(bia.person, 'ana', 0, 5000);

    const before = await sfu.totals();

    bia.cut();
    await sleep(OUTAGE_MS);

    const restoredAt = Date.now();

    bia.restore();

    const back = await backAfter(bia.person, 'ana', restoredAt, 20_000);
    const after = await settled(before, 'server back to the same objects');

    await sleep(3000);

    const route = screenRoute(bia.person, 'ana');

    route.watch.reset();
    await sleep(3000);

    const steady = route.watch.summary();

    record('g-assiste', { outageMs: OUTAGE_MS, backAfterMs: back, joins: bia.person.joins, events: bia.person.history.filter(entry => ['closed', 'signalingMuted', 'rejoined', 'rewatch', 'arrivalDead'].includes(entry.event)).map(entry => entry.event), before, after, steady, peerLeftSeenByAna: ana.events('peerLeft').length });

    assert.ok(back <= 15_000, `voltou a ver ${back} ms depois da rede voltar`);
    assert.deepEqual({ transports: after.transports, producers: after.producers, consumers: after.consumers }, { transports: before.transports, producers: before.producers, consumers: before.consumers });
    assert.deepEqual(cleanVideo(steady, SCREEN), []);
    assert.equal(ana.events('peerLeft').length, 0, 'a sala viu a Bia sair');
});

test('g. quem transmite perde a rede por 10 s: a tela volta para quem assiste, e o servidor não guarda nada a mais', async () => {
    const room = 'e2e-rede-transmite';
    const ana = await unplugged(room, 'ana', { watch: false });
    const bia = await new Participant({ name: 'bia', url: sfu.url, identity: guest(room, 'bia') }).join();

    people.push(bia);

    await ana.person.publish('screen', SCREEN);
    await ana.person.publish('mic');
    await backAfter(bia, 'ana', 0, 5000);

    const before = await sfu.totals();

    ana.cut();
    await sleep(OUTAGE_MS);

    const restoredAt = Date.now();

    ana.restore();

    const back = await backAfter(bia, 'ana', restoredAt, 20_000);
    const after = await settled(before, 'server back to the same objects');

    await sleep(3000);

    const route = screenRoute(bia, 'ana');

    route.watch.reset();
    await sleep(3000);

    const steady = route.watch.summary();

    record('g-transmite', { outageMs: OUTAGE_MS, backAfterMs: back, joins: ana.person.joins, events: ana.person.history.filter(entry => ['closed', 'signalingMuted', 'rejoined', 'republish', 'lostTheServer'].includes(entry.event)).map(entry => entry.event), before, after, steady });

    assert.ok(back <= 15_000, `a tela voltou ${back} ms depois da rede voltar`);
    assert.deepEqual({ transports: after.transports, producers: after.producers, consumers: after.consumers }, { transports: before.transports, producers: before.producers, consumers: before.consumers });
    assert.deepEqual(cleanVideo(steady, SCREEN), []);
});

test(`g. ${CYCLES} ciclos de entrar, transmitir, assistir e sair: nada vaza e a memória fica estável`, async () => {
    const room = 'e2e-ciclos';
    const ana = await new Participant({ name: 'ana', url: sfu.url, identity: guest(room, 'ana'), watch: false, gate: 'immediate' }).join();

    people.push(ana);

    await ana.publish('screen', SMALL);
    await ana.publish('mic');
    await sleep(1000);

    const baseline = await sfu.totals();
    const samples = [];

    for (let cycle = 1; cycle <= CYCLES; cycle += 1) {
        const person = new Participant({ name: `c${cycle}`, url: sfu.url, identity: guest(room, `c${cycle}`), gate: 'immediate' });

        // Ciclo que falha não pode deixar a pessoa tentando voltar para sempre.
        people.push(person);
        await person.join();
        await person.publish('mic');

        if (cycle % 4 === 0) {
            await person.publish('camera', SMALL);
        }

        await waitFor(() => person.receiver && [...person.receiver.routes.values()].some(route => route.packets > 0), 3000, `cycle ${cycle} receiving`);

        // Um em quatro cai sem `leave` (app fechado): fica na carência de 30 s e sai sozinho.
        if (cycle % 4 === 1) {
            person.crash();
        } else {
            await person.leave();
        }

        people.pop();

        if (cycle % Math.max(1, Math.floor(CYCLES / 4)) === 0) {
            samples.push({ cycle, ...(await sfu.memory()) });
        }
    }

    const cleaned = await waitFor(async () => {
        const totals = await sfu.totals();

        return totals.peers === baseline.peers && totals.transports === baseline.transports && totals.producers === baseline.producers && totals.consumers === baseline.consumers ? totals : null;
    }, 45_000, 'every cycle released', 1000).catch(async () => sfu.totals());
    // A memória se compara no mesmo estado (logo depois de um ciclo, com a carência dos que
    // caíram em curso): do primeiro quarto ao último.
    const growth = samples.at(-1).totalKb - samples[0].totalKb;

    record('g-ciclos', { cycles: CYCLES, baseline, cleaned, samples, settledMemory: await sfu.memory(), growthKb: growth });

    const shape = totals => ({ rooms: totals.rooms, peers: totals.peers, routers: totals.routers, transports: totals.transports, producers: totals.producers, consumers: totals.consumers });

    assert.deepEqual(shape(cleaned), shape(baseline));
    // Mais que 10% (ou 30 MB) a mais do primeiro quarto ao fim é coisa que não volta.
    assert.ok(growth <= Math.max(samples[0].totalKb * 0.1, 30_000), `a memória cresceu ${growth} KB do ciclo ${samples[0].cycle} ao ${samples.at(-1).cycle}`);
});
