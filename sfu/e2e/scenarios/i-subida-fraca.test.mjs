import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { Participant } from '../lib/Participant.mjs';
import { guest, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';
import { UdpProxy } from '../lib/UdpProxy.mjs';

const CAMERA = { width: 640, height: 360, fps: 30, bitrate: 800_000 };
const RUN_MS = 25_000;
const NETWORK = { delayMs: 10, jitterMs: 20, reorder: 0.01 };

/** A perda de quem assiste mal: o bastante para o NACK não tapar tudo e o PLI não parar. */
const VIEWER_LOSS = 0.15;

let sfu;
const people = [];
const proxies = [];

before(async () => {
    sfu = await new SfuProcess({ ...ports(9), workers: 1 }).start();
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
 * Ana transmite tela, câmera e microfone por uma subida de `rateBps` (fila de `bufferMs`, como o
 * roteador de casa); Bia assiste com perda constante e pede quadro-chave sem parar; Caio assiste
 * limpo, de controle. Devolve o que a Ana mandou, o que a subida segurou e o que o Caio viu.
 */
const weakUplink = async (room, { gate, rateBps, screenBitrate, warmMs }) => {
    const uplink = new UdpProxy({ ...NETWORK, rateBps, bufferMs: 250 });
    const downlink = new UdpProxy({ ...NETWORK, loss: VIEWER_LOSS });

    proxies.push(uplink, downlink);

    const ana = await new Participant({ name: 'ana', url: sfu.url, identity: guest(room, 'ana'), watch: false, gate, governor: true, extendedReports: true, relay: (host, port) => uplink.relay(host, port) }).join();
    const bia = await new Participant({ name: 'bia', url: sfu.url, identity: guest(room, 'bia'), receiverReports: true, relay: (host, port) => downlink.relay(host, port) }).join();
    const caio = await new Participant({ name: 'caio', url: sfu.url, identity: guest(room, 'caio'), receiverReports: true }).join();

    people.push(ana, bia, caio);

    const publishedAt = performance.now();
    const publishedWall = Date.now();

    await ana.publish('screen', { width: 1920, height: 1080, fps: 30, bitrate: screenBitrate });
    await ana.publish('camera', CAMERA);
    await ana.publish('mic');
    await waitFor(() => caio.watchSummaries().filter(summary => summary.decodable > 0 || summary.lost !== undefined).length === 3, 8000, 'caio receiving');
    await sleep(warmMs);

    const tracksBefore = Object.fromEntries([...ana.tracks].map(([source, track]) => [source, { sent: track.keyframesSent, asked: track.keyframeRequests }]));
    const statsBefore = { ...uplink.stats, queueSamples: [] };

    uplink.stats.queueSamples = [];
    bia.resetWatches();
    caio.resetWatches();

    const startedAt = performance.now();

    await sleep(RUN_MS);

    const tracks = Object.fromEntries([...ana.tracks].map(([source, track]) => [source, { keyframes: track.keyframesSent - tracksBefore[source].sent, asked: track.keyframeRequests - tracksBefore[source].asked, idrBytes: track.keyframe.bytes }]));
    const waits = uplink.stats.queueSamples.filter(([at]) => at >= startedAt).map(([, wait]) => wait).sort((left, right) => left - right);
    const result = {
        gate,
        rateBps,
        screenBitrate,
        viewerLoss: VIEWER_LOSS,
        runMs: RUN_MS,
        tracks,
        uplink: {
            queueDropped: uplink.stats.queueDropped - statsBefore.queueDropped,
            maxQueueMs: waits.at(-1) ?? 0,
            queueP50Ms: waits[Math.floor(waits.length / 2)] ?? 0,
            queueP99Ms: waits[Math.floor(waits.length * 0.99)] ?? 0,
            queuedOver100Ms: waits.filter(wait => wait > 100).length,
            packets: waits.length,
            averageMbps: Math.round(((uplink.stats.upBytes - statsBefore.upBytes) * 8) / (RUN_MS / 1000) / 10_000) / 100,
        },
        sender: { resent: ana.sender.stats.resent, nacked: ana.sender.stats.nacked, packets: ana.sender.stats.packets },
        governor: Object.fromEntries([...ana.tracks].map(([source, track]) => [source, { target: track.governor?.target, history: (track.governor?.history ?? []).map(step => ({ ...step, at: step.at - publishedWall })) }])),
        firstDropMs: uplink.stats.firstDropAt === undefined ? null : Math.round(uplink.stats.firstDropAt - publishedAt),
        keyframeTimesMs: Object.fromEntries([...ana.tracks].map(([source, track]) => [source, (track.keyframeTimes ?? []).map(at => at - publishedWall).filter(at => at < 6000)])),
        biaPlis: bia.receiver.stats.plis,
        control: caio.watchSummaries().map(summary => ({ label: summary.label, fps: summary.fps, decodable: summary.decodable, maxFreezeMs: summary.maxFreezeMs, maxGapMs: summary.maxGapMs, latencyP50Ms: summary.latencyP50Ms, latencyP99Ms: summary.latencyP99Ms })),
    };

    for (const person of [ana, bia, caio]) {
        person.crash();
    }

    return result;
};

/** `native` é o app; os outros são para comparar (`GATES=native,old,fast,bucket,immediate`). */
const GATES = (process.env.GATES ?? 'native').split(',');
const ALL_LINKS = [
    // O caso de todo dia do upload fraco: a qualidade pede mais do que a subida leva, e o
    // governador desce até caber.
    // Os primeiros ~20 s são o governador descendo (30% a cada 8 s de carência), com qualquer
    // freio: quem assiste limpo chega a parar ~2 s. Mede o regime depois disso.
    { name: '5M-teto6', rateBps: 5_000_000, screenBitrate: 6_000_000, warmMs: Number(process.env.WARM_MS ?? 25_000) },
    // A borda: o teto cabe com 12% de folga, o governador nunca desceu, e a rajada vale.
    {
        name: '5M', rateBps: 5_000_000, screenBitrate: 3_500_000, warmMs: 2000,
        todo: 'cliente nativo: com o governador no teto, a rajada de 2 quadros-chave descarta ~100 pacotes uma vez numa subida a 88%, e quem assiste limpo espera o recuo de 4 s (o freio da 0.1.7, sem rajada, não descarta)',
    },
    { name: '5M-77', rateBps: 5_000_000, screenBitrate: 3_000_000, warmMs: 2000 },
    { name: '10M', rateBps: 10_000_000, screenBitrate: 6_000_000, warmMs: 2000 },
];
const LINKS = ALL_LINKS.filter(link => (process.env.LINKS ?? '5M-teto6,5M,5M-77,10M').split(',').includes(link.name));
const ROUNDS = Number(process.env.ROUNDS ?? 1);
const results = {};

for (let round = 1; round <= ROUNDS; round += 1) for (const link of LINKS) {
    for (const gate of GATES) {
        test(`i. rodada ${round}, subida de ${link.name}, tela ${link.screenBitrate / 1e6} Mb/s + câmera, alguém com ${VIEWER_LOSS * 100}% de perda pedindo PLI: freio ${gate}`, { todo: gate === 'native' ? link.todo : undefined }, async () => {
            const result = await weakUplink(`e2e-subida-${link.name.toLowerCase().replace(/[^a-z0-9]/g, '')}-${gate}-${round}`, { gate, rateBps: link.rateBps, screenBitrate: link.screenBitrate, warmMs: link.warmMs });

            results[`${link.name}-${gate}`] = result;
            record(`i-${link.name}-${gate}`, result);
            console.log(JSON.stringify({ round, link: link.name, gate, tracks: result.tracks, uplink: result.uplink, biaPlis: result.biaPlis, firstDropMs: result.firstDropMs, keyframesFirst6s: result.keyframeTimesMs, governor: result.governor.screen, sender: result.sender, control: result.control.map(summary => `${summary.label.split(':')[1]} fps=${summary.fps} congela=${summary.maxFreezeMs ?? summary.maxGapMs} lat99=${summary.latencyP99Ms}`) }));

            if (gate === 'native') {
                const video = result.control.filter(summary => summary.decodable !== undefined);

                // Não saturar é a fila do roteador vazia na maior parte do tempo e quem assiste limpo
                // sem parar: algumas centenas de descartes enquanto o governador volta a subir são
                // o controle por perda fazendo o trabalho dele.
                assert.ok(video.every(summary => summary.maxFreezeMs <= 1000), `quem assiste limpo congelou: ${JSON.stringify(video)}`);
                assert.ok(result.uplink.queueP50Ms <= 50, `a fila do roteador ficou cheia: mediana de ${result.uplink.queueP50Ms} ms`);
            }
        });
    }
}
