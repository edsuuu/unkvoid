// P1 — cada `resumeConsumer` de vídeo custa DOIS quadros-chave, um na hora e outro 1 s depois.
// `ConsumerController.resume` chama `consumer.resume()` (que o mediasoup já faz pedir quadro-chave)
// e logo depois `consumer.requestKeyFrame()`, que cai dentro do `keyFrameRequestDelay` de 1 s do
// producer e vira um segundo PLI agendado. Quem transmite (app nativo) só manda quadro-chave
// quando pedem (gop 0), como o Windows entre os GOPs de 4 s.
//
// node sfu/audit/double-keyframe.mjs      (FAIL = defeito reproduzido)
import { check, joinGuest, publish, sleep, startSfu, watch } from './lib.mjs';

const sfu = await startSfu({ base: 3471 });
let failures = 0;

try {
    const alice = await joinGuest(sfu, 'firstpic', 'alice');
    const SSRC = 0x44440001;
    const { answer: produced, sender } = await publish(alice, 'screen', SSRC);
    const timer = setInterval(() => sender.frame(SSRC, { gop: 0 }), 16);

    for (const [label, delay] of [['primeiro espectador', 1500], ['segundo espectador, 6 s depois', 6000]]) {
        await sleep(delay);
        const viewer = await joinGuest(sfu, 'firstpic', `v${delay}`);
        const before = sender.plis(SSRC).length;
        const { answer, receiver } = await watch(viewer, produced.producerId);
        const resumedAt = Date.now();
        let first = null;

        while (Date.now() - resumedAt < 2500) {
            if (!first && receiver.of(answer.ssrc).length > 0) first = Date.now() - resumedAt;
            await sleep(5);
        }

        const plis = sender.plis(SSRC).slice(before).map((entry) => entry.at - resumedAt);

        console.log(`${label}: PLIs (ms depois do resume) ${JSON.stringify(plis)}; primeiro pacote em ${first ?? '—'} ms`);
        check(`${label}: um resume pede UM quadro-chave`, plis.length === 1, `${plis.length} PLIs`) || failures++;
    }

    clearInterval(timer);
    sender.close();
} finally {
    await sfu.stop();
}
console.log(failures ? `\n${failures} check(s) failed (FAIL = bug reproduced)` : '\nall checks passed');
process.exit(failures ? 1 : 0);
