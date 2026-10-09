// Carga: 12 pessoas numa sala (passa do `SFU_PEERS_PER_ROUTER` de 10, então as duas últimas
// caem num router de outro worker e o que elas assistem chega por `pipeToRouter`), 2 telas — uma
// em cada router — e o microfone de todo mundo, cada um assistindo a tudo dos outros pelo RTP puro
// com SRTP, como o app nativo. Confere que cada espectador recebe as duas telas começando por um
// quadro-chave e o microfone de cada um dos outros.
//
// node sfu/audit/load-ten.mjs      (FAIL = defeito reproduzido)
import { check, joinGuest, newKey, publish, sleep, startSfu, watch } from './lib.mjs';

const PEOPLE = 12;
const sfu = await startSfu({ base: 3491, workers: 2, plainPorts: 64, peersPerRouter: 10 });
let failures = 0;

try {
    const people = [];

    for (let index = 0; index < PEOPLE; index += 1) {
        people.push(await joinGuest(sfu, 'carga', `p${index}`));
    }

    const timers = [];
    const producers = [];

    for (const [index, client] of people.entries()) {
        const key = newKey();
        const mic = 0x60000000 + index * 16;
        const { answer: voice, sender } = await publish(client, 'mic', mic, key);

        producers.push({ owner: index, id: voice.producerId, kind: 'mic' });
        timers.push(setInterval(() => sender.sendOpus(mic), 20));

        if (index === 0 || index === PEOPLE - 1) {
            const screen = mic + 1;
            const { answer: shared } = await publish(client, 'screen', screen, key, sender);

            producers.push({ owner: index, id: shared.producerId, kind: 'screen' });
            timers.push(setInterval(() => sender.frame(screen, { gop: 0 }), 33));
        }
    }

    const watchers = [];

    for (const [index, client] of people.entries()) {
        let receiver = null;
        const watched = [];

        for (const producer of producers.filter((entry) => entry.owner !== index)) {
            const { answer, receiver: opened } = await watch(client, producer.id, receiver);

            receiver = opened;
            watched.push({ ...producer, ssrc: answer.ssrc });
        }

        watchers.push({ index, receiver, watched });
    }

    await sleep(5000);

    let screensOk = 0;
    let micsOk = 0;
    const problems = [];

    for (const { index, receiver, watched } of watchers) {
        for (const producer of watched) {
            const got = receiver.of(producer.ssrc);

            if (producer.kind === 'screen') {
                if (got.length > 20 && got[0].nal === 7) {
                    screensOk += 1;
                } else {
                    problems.push(`p${index} ← tela de p${producer.owner}: ${got.length} pacotes, primeiro nal ${got[0]?.nal}`);
                }
            } else if (got.length > 100) {
                micsOk += 1;
            } else {
                problems.push(`p${index} ← mic de p${producer.owner}: ${got.length} pacotes`);
            }
        }
    }

    const screensWanted = watchers.reduce((sum, entry) => sum + entry.watched.filter((producer) => producer.kind === 'screen').length, 0);
    const micsWanted = watchers.reduce((sum, entry) => sum + entry.watched.filter((producer) => producer.kind === 'mic').length, 0);

    check(`cada espectador recebe as 2 telas (inclusive entre routers), começando por quadro-chave`, screensOk === screensWanted, `${screensOk}/${screensWanted}`) || failures++;
    check(`cada espectador recebe o microfone de todos os outros`, micsOk === micsWanted, `${micsOk}/${micsWanted}`) || failures++;

    for (const problem of problems.slice(0, 10)) {
        console.log(`   ${problem}`);
    }

    for (const timer of timers) clearInterval(timer);
    for (const client of people) client.close();
} finally {
    await sfu.stop();
}
console.log(failures ? `\n${failures} check(s) failed (FAIL = bug reproduced)` : '\nall checks passed');
process.exit(failures ? 1 : 0);
