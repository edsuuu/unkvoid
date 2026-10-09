import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { Participant } from '../lib/Participant.mjs';
import { decode, guest, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';
import { UdpProxy } from '../lib/UdpProxy.mjs';

const SCREEN = { width: 1920, height: 1080, fps: 30, bitrate: 6_000_000 };
const CAMERA = { width: 640, height: 360, fps: 30, bitrate: 800_000 };
const LOSS = 0.05;
const RUN_MS = 15_000;

/** A ida até São Paulo (~10 ms) com variação de até 20 ms, e 1% dos pacotes fora de ordem. */
const NETWORK = { delayMs: 10, jitterMs: 20, reorder: 0.01 };

let sfu;
const people = [];
const proxies = [];

before(async () => {
    sfu = await new SfuProcess({ ...ports(2), workers: 2 }).start();
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

/**
 * Ana transmite tela, câmera e microfone pela `uplink`; Bia assiste pela `downlink`; Caio
 * assiste sem perda, de controle. Devolve o que a Bia viu numa janela de `RUN_MS`.
 */
const lossyCall = async (room, { uplinkLoss, downlinkLoss, viewer }) => {
    const uplink = new UdpProxy({ ...NETWORK, loss: uplinkLoss });
    const downlink = new UdpProxy({ ...NETWORK, loss: downlinkLoss });

    proxies.push(uplink, downlink);

    const ana = await new Participant({ name: 'ana', url: sfu.url, identity: guest(room, 'ana'), watch: false, gate: viewer.gate, extendedReports: viewer.extendedReports, relay: (host, port) => uplink.relay(host, port) }).join();
    const bia = await new Participant({ name: 'bia', url: sfu.url, identity: guest(room, 'bia'), keepStreams: true, receiverReports: viewer.receiverReports, relay: (host, port) => downlink.relay(host, port) }).join();
    const caio = await new Participant({ name: 'caio', url: sfu.url, identity: guest(room, 'caio') }).join();

    people.push(ana, bia, caio);

    await ana.publish('screen', SCREEN);
    await ana.publish('camera', CAMERA);
    await ana.publish('mic');
    await waitFor(() => bia.watchSummaries().filter(summary => summary.decodable > 0 || summary.lost !== undefined).length === 3, 8000, 'bia receiving');

    bia.resetWatches();
    caio.resetWatches();
    await sleep(RUN_MS);

    const watched = bia.watchSummaries();
    const screen = watched.find(summary => summary.label.endsWith(':screen'));
    const result = {
        uplinkLoss,
        downlinkLoss,
        ...NETWORK,
        viewer,
        runMs: RUN_MS,
        watched,
        control: caio.watchSummaries(),
        recovery: [...bia.receiver.routes.values()].filter(route => route.video).map(route => ({ label: route.watch.label, ...route.recovery.counters })),
        uplink: uplink.stats,
        downlink: downlink.stats,
        sender: ana.sender.stats,
        receiver: bia.receiver.stats,
        decoded: decode(bia.watchOf(screen.producerId).annexB(), `${room}-tela`),
    };

    for (const person of [ana, bia, caio]) {
        person.crash();
    }

    return result;
};

const problemsOf = (result, freezeLimitMs = 1000) => {
    const problems = [];

    for (const summary of result.watched) {
        if (summary.decodable !== undefined) {
            if (summary.maxFreezeMs > freezeLimitMs) {
                problems.push(`${summary.label}: imagem parada por ${summary.maxFreezeMs} ms`);
            }

            if (summary.outOfOrder > 0) {
                problems.push(`${summary.label}: ${summary.outOfOrder} quadros fora de ordem`);
            }

            if (summary.decodable < (RUN_MS / 1000) * SCREEN.fps * 0.8) {
                problems.push(`${summary.label}: só ${summary.decodable} quadros em ${RUN_MS / 1000} s`);
            }
        } else if (summary.maxGapMs > 500) {
            // Fora de ordem, aqui, é o 1% que a rede embaralha: o som não tem fila de reordenação
            // no app (`receiver.rs` repassa direto) e o Opus esconde o pacote que falta.
            problems.push(`${summary.label}: som parado ${summary.maxGapMs} ms`);
        }
    }

    if (result.decoded.errors !== '') {
        problems.push(`o ffmpeg reclamou: ${result.decoded.errors}`);
    }

    return problems;
};

/**
 * O cliente com o que falta ao app de hoje: quem assiste manda RR, quem transmite responde o
 * RRTR com DLRR e atende o pedido de quadro-chave na hora. É assim que se mede o SFU sozinho.
 */
const FIXED_CLIENT = { receiverReports: true, extendedReports: true, gate: 'immediate' };

test('c. 5% de perda e jitter na subida (quem transmite → SFU): o SFU pede de volta e a imagem não para mais de 1 s', async () => {
    const result = await lossyCall('e2e-perda-subida', { uplinkLoss: LOSS, downlinkLoss: 0, viewer: FIXED_CLIENT });

    record('c-subida', result);

    assert.deepEqual(problemsOf(result), []);
    assert.ok(result.sender.resent > 0, 'o SFU não pediu reenvio a quem transmite');
});

test('c. 5% de perda e jitter na descida (SFU → quem assiste): o RTX do SFU recupera e a imagem não para mais de 1 s', async () => {
    const result = await lossyCall('e2e-perda-descida', { uplinkLoss: 0, downlinkLoss: LOSS, viewer: FIXED_CLIENT });

    record('c-descida', result);

    assert.deepEqual(problemsOf(result), []);
    assert.ok(result.recovery.every(counter => counter.recovered > 0), 'o RTX não recuperou nada');
});

// O dobro do pedido (≈10% de ponta a ponta): o buraco raro que sobra passa pelo prazo do
// receptor e pelo PLI duas vezes, e a parada encosta em 1 s. O teto aqui é 1,5 s.
test('c. 5% nos dois lados (≈10% de ponta a ponta): a imagem não para mais de 1,5 s', async () => {
    const result = await lossyCall('e2e-perda-dois-lados', { uplinkLoss: LOSS, downlinkLoss: LOSS, viewer: FIXED_CLIENT });

    record('c-dois-lados', result);

    assert.deepEqual(problemsOf(result, 1500), []);
});

test(
    'c. o app de hoje com 5% na descida: sem RR o mediasoup ignora o NACK repetido, e cada buraco espera o freio de 2 s',
    { todo: 'cliente nativo: receptor sem RTCP RR (receiver.rs), remetente sem XR DLRR (plain.rs) e KEYFRAME_SPACING de 2 s (sharing.rs)' },
    async () => {
        const result = await lossyCall('e2e-perda-app', { uplinkLoss: 0, downlinkLoss: LOSS, viewer: { receiverReports: false, extendedReports: false, gate: 'native' } });

        record('c-app-de-hoje', result);

        assert.deepEqual(problemsOf(result), []);
    },
);
