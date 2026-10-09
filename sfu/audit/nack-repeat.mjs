// P2 — o receptor nativo pede o mesmo pacote até três vezes, de 40 em 40 ms quando a ida e volta
// é curta (`native/shared/media/src/recovery.rs`: RETRY_FLOOR). O mediasoup não reenvia o mesmo
// pacote dentro de uma ida e volta, e sem relatório de recepção do receptor (o `PlainReceiver`
// não manda RR) ela vale 100 ms (`RtpStreamSend.cpp`: DefaultRtt). Conta quantos RTX voltam para
// três NACKs do mesmo número a 0, 40 e 80 ms — e para três a 0, 120 e 240 ms.
//
// node sfu/audit/nack-repeat.mjs      (FAIL = defeito reproduzido)
import { check, joinGuest, publish, sleep, startSfu, watch } from './lib.mjs';

const sfu = await startSfu({ base: 3481 });
let failures = 0;

try {
    const alice = await joinGuest(sfu, 'nackrepeat', 'alice');
    const bob = await joinGuest(sfu, 'nackrepeat', 'bob');
    const SSRC = 0x55550001;
    const { answer: produced, sender } = await publish(alice, 'screen', SSRC);
    const timer = setInterval(() => sender.frame(SSRC, { gop: 30 }), 16);

    await sleep(1500);

    const { answer, receiver } = await watch(bob, produced.producerId);

    await sleep(1500);

    for (const spacing of [40, 120]) {
        const target = receiver.of(answer.ssrc).at(-5);
        const resent = () => receiver.of(answer.rtx.ssrc).filter((entry) => entry.payload.readUInt16BE(0) === target.seq).length;
        const before = resent();

        for (let ask = 0; ask < 3; ask += 1) {
            receiver.nack(answer.ssrc, target.seq);
            await sleep(spacing);
        }

        await sleep(200);
        check(`3 NACKs do mesmo pacote a cada ${spacing} ms trazem 3 reenvios`, resent() - before === 3, `${resent() - before} reenvio(s)`) || failures++;
    }

    clearInterval(timer);
    sender.close();
    receiver.close();
} finally {
    await sfu.stop();
}
console.log(failures ? `\n${failures} check(s) failed (FAIL = bug reproduced)` : '\nall checks passed');
process.exit(failures ? 1 : 0);
