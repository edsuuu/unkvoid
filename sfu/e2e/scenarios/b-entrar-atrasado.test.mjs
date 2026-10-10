import assert from 'node:assert/strict';
import { after, afterEach, before, test } from 'node:test';

import { Participant } from '../lib/Participant.mjs';
import { guest, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';
import { UdpProxy } from '../lib/UdpProxy.mjs';

const SCREEN = { width: 1920, height: 1080, fps: 30, bitrate: 6_000_000 };
const CAMERA = { width: 640, height: 360, fps: 30, bitrate: 800_000 };
const LIMIT_MS = 1000;

/** Sem GOP periódico: o quadro do atrasado só pode vir do pedido dele (PLI/FIR). */
const NO_GOP = 600_000;

let sfu;
const people = [];
const proxies = [];

before(async () => {
    sfu = await new SfuProcess({ ...ports(1), workers: 2 }).start();
});

after(async () => {
    for (const person of people) {
        person.crash();
    }

    for (const proxy of proxies) {
        proxy.close();
    }

    await sfu?.stop();
});

// Cada teste solta quem abriu: quem transmitia 1080p num teste anterior seguia mandando para quem
// assistia, no mesmo processo do harness, e os últimos testes mediam a fila do harness, não o SFU.
afterEach(() => {
    for (const person of people.splice(0)) {
        person.crash();
    }

    for (const proxy of proxies.splice(0)) {
        proxy.close();
    }
});

/** Quem transmite: tela, câmera e microfone, com o freio de quadro-chave pedido. */
const broadcaster = async (room, options, gopMs = NO_GOP) => {
    const ana = await new Participant({ name: 'ana', url: sfu.url, identity: guest(room, 'ana'), watch: false, ...options }).join();

    people.push(ana);

    for (const track of ['screen', 'camera']) {
        await ana.publish(track, { ...(track === 'screen' ? SCREEN : CAMERA), gopMs });
    }

    await ana.publish('mic');
    await sleep(3000);

    return ana;
};

/** Entra atrasado e mede do clique (antes do WebSocket abrir) ao primeiro quadro decodificável. */
const lateJoin = async (room, name, options = {}) => {
    const startedAt = Date.now();
    const person = await new Participant({ name, url: sfu.url, identity: guest(room, name), ...options }).join();

    people.push(person);

    const firsts = await waitFor(
        () => {
            const videos = [...person.receiver?.routes.values() ?? []].filter(route => route.video);

            return videos.length === 2 && videos.every(route => route.watch.firstDecodableAt !== null)
                ? Object.fromEntries(videos.map(route => [route.watch.label.split(':')[1], route.watch.firstDecodableAt - startedAt]))
                : null;
        },
        6000,
        `${name} first frames`,
    ).catch(() => {
        const videos = [...person.receiver?.routes.values() ?? []].filter(route => route.video);

        return Object.fromEntries(videos.map(route => [route.watch.label.split(':')[1], route.watch.firstDecodableAt === null ? null : route.watch.firstDecodableAt - startedAt]));
    });

    return { name, ...firsts };
};

const overLimit = joins => joins.flatMap(join => ['screen', 'camera'].filter(source => join[source] === null || join[source] === undefined || join[source] > LIMIT_MS).map(source => `${join.name} ${source}: ${join[source]} ms`));

test('b. quem entra atrasado vê o primeiro quadro em até 1 s, só pelo pedido de quadro-chave (o SFU sozinho)', async () => {
    const room = 'e2e-atrasado';
    const ana = await broadcaster(room, { gate: 'immediate' });
    const joins = [];

    // Um por vez, longe do freio de 1 s do `keyFrameRequestDelay`.
    for (const name of ['l1', 'l2', 'l3']) {
        joins.push(await lateJoin(room, name));
        await sleep(1500);
    }

    // Dois a 300 ms um do outro: o segundo cai dentro do freio do mediasoup.
    const first = lateJoin(room, 'p1');

    await sleep(300);
    joins.push(await first, await lateJoin(room, 'p2'));
    await sleep(1500);

    // Três juntos (o link colado no grupo).
    joins.push(...(await Promise.all(['g1', 'g2', 'g3'].map(name => lateJoin(room, name)))));

    record('b', { gate: 'immediate', joins, keyframes: ana.tracks.get('screen').keyframesSent, keyframeRequests: ana.sender.stats.keyframeRequests });

    assert.deepEqual(overLimit(joins), []);
});

test('b. cada atrasado custa um quadro-chave por vídeo a quem transmite, não dois', async () => {
    const room = 'e2e-atrasado-custo';
    const ana = await broadcaster(room, { gate: 'immediate' });
    const costs = [];

    for (const name of ['k1', 'k2', 'k3']) {
        const before = { screen: ana.tracks.get('screen').keyframesSent, camera: ana.tracks.get('camera').keyframesSent };
        const join = await lateJoin(room, name);

        // Passado o freio de 1 s do mediasoup, todo pedido atrasado já teria saído.
        await sleep(2500);

        costs.push({
            ...join,
            screenKeyframes: ana.tracks.get('screen').keyframesSent - before.screen,
            cameraKeyframes: ana.tracks.get('camera').keyframesSent - before.camera,
        });
    }

    // Logo depois do freio de quem entrou antes: com o pedido em dobro, este esperava o segundo.
    const first = await lateJoin(room, 'k4');

    await sleep(1100);

    const right = await lateJoin(room, 'k5');

    record('b-custo', { costs, afterAnother: [first, right] });

    assert.deepEqual(
        costs.filter(cost => cost.screenKeyframes !== 1 || cost.cameraKeyframes !== 1).map(cost => `${cost.name}: tela ${cost.screenKeyframes}, câmera ${cost.cameraKeyframes}`),
        [],
    );
    assert.deepEqual(overLimit([first, right]), []);
});

test('b. quem entra logo depois do quadro-chave de outro (dentro do freio do mediasoup) ainda vê em até 1 s', async () => {
    const room = 'e2e-atrasado-freio';

    // A ida até São Paulo (15 ms por sentido): no localhost o pior caso fica rente ao limite.
    const network = new UdpProxy({ delayMs: 15 });

    proxies.push(network);
    const relay = { relay: (host, port) => network.relay(host, port) };

    await broadcaster(room, { gate: 'immediate', ...relay });

    const pairs = [];

    for (let pair = 0; pair < 5; pair += 1) {
        const first = await lateJoin(room, `f${pair}`, relay);
        // O pedido do segundo chega quando o quadro-chave do primeiro já passou: é o pior caso
        // do `keyFrameRequestDelay`, que segura o pedido até o freio acabar.
        const second = await lateJoin(room, `s${pair}`, relay);

        pairs.push(first, second);
        await sleep(1500);
    }

    record('b-freio', { pairs });

    assert.deepEqual(overLimit(pairs), []);
});

test('b. com o freio de quadro-chave do app (2 s), o segundo atrasado espera mais de 1 s', async () => {
    const room = 'e2e-atrasado-app';

    await broadcaster(room, { gate: 'native' });

    const first = lateJoin(room, 'n1');

    await sleep(300);

    const joins = [await first, await lateJoin(room, 'n2')];

    record('b-gate-nativo', { gate: 'native', joins });

    assert.deepEqual(overLimit(joins), []);
});

test('b. com o pedido de quadro-chave dividido entre tela e câmera (o app de hoje), a câmera espera o GOP', async () => {
    const room = 'e2e-atrasado-ssrc';

    // O GOP de 4 s do encoder do Windows: é ele que acaba trazendo a câmera.
    await broadcaster(room, { gate: 'native', keyframeRouting: 'ssrc' }, 4000);

    const joins = [await lateJoin(room, 's1')];

    await sleep(1500);
    joins.push(await lateJoin(room, 's2'));

    record('b-pli-dividido', { keyframeRouting: 'shared', joins });

    assert.deepEqual(overLimit(joins), []);
});

for (const gate of (process.env.APP_GATES ?? 'native').split(',')) {
    test(`b. app (${gate}): um por vez, dois a 300 ms e três juntos veem em até 1 s com o freio do app`, async () => {
        const room = `e2e-app-seq-${gate}`;
        const mark = people.length;
        const ana = await broadcaster(room, { gate, governor: true });
        const joins = [];

        for (const name of ['l1', 'l2', 'l3']) {
            joins.push(await lateJoin(room, name));
            await sleep(1500);
        }

        const first = lateJoin(room, 'p1');

        await sleep(300);
        joins.push(await first, await lateJoin(room, 'p2'));
        await sleep(1500);
        joins.push(...(await Promise.all(['g1', 'g2', 'g3'].map(name => lateJoin(room, name)))));

        record(`b-app-seq-${gate}`, { gate, joins, keyframes: ana.tracks.get('screen').keyframesSent });
        console.log(JSON.stringify({ test: 'seq', gate, joins, keyframes: ana.tracks.get('screen').keyframesSent }));
        people.splice(mark).forEach(person => person.crash());

        assert.deepEqual(overLimit(joins), []);
    });

    test(`b. app (${gate}): cinco pares, o segundo logo depois do quadro-chave do primeiro, veem em até 1 s com o freio do app`, async () => {
        const room = `e2e-app-pares-${gate}`;
        const mark = people.length;
        const network = new UdpProxy({ delayMs: 15 });

        proxies.push(network);
        const relay = { relay: (host, port) => network.relay(host, port) };

        await broadcaster(room, { gate, governor: true, ...relay });

        const pairs = [];

        for (let pair = 0; pair < 5; pair += 1) {
            const first = await lateJoin(room, `f${pair}`, relay);
            const second = await lateJoin(room, `s${pair}`, relay);

            pairs.push(first, second);
            await sleep(1500);
        }

        record(`b-app-pares-${gate}`, { gate, pairs });
        console.log(JSON.stringify({ test: 'pares', gate, pairs }));
        people.splice(mark).forEach(person => person.crash());

        assert.deepEqual(overLimit(pairs), []);
    });
}
