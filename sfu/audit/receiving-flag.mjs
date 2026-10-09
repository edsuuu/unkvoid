// T02 — `producerReceiving` / `consumePlain.receiving` vs reality.
//  (a) publisher goes silent: contract says the flag drops ~1.5 s later. Does it?
//  (b) publisher keeps sending but the producer is paused (self-mute, /mute): the SFU
//      forwards nothing, yet tells watchers `receiving: true`.
import { check, joinGuest, log, publish, sleep, startSfu, watch } from './lib.mjs';

const sfu = await startSfu({ base: 3403 });
let failures = 0;
const ok = (...args) => check(...args) || failures++;

try {
    const alice = await joinGuest(sfu, 'flag', 'alice');
    const bob = await joinGuest(sfu, 'flag', 'bob');

    // (a) screen that stops arriving at the SFU
    const SCREEN = 0x22220001;
    const screen = await publish(alice, 'screen', SCREEN);
    screen.sender.timer = setInterval(() => !screen.sender.stopped && screen.sender.frame(SCREEN), 33);
    await bob.waitEvent('producerReceiving', (data) => data.producerId === screen.answer.producerId && data.receiving, 5000);
    const watched = await watch(bob, screen.answer.producerId);
    await sleep(1500);
    ok('watcher gets the screen while it flows', watched.receiver.of(watched.answer.ssrc).length > 10);

    screen.sender.stopped = true;
    const stoppedAt = Date.now();
    const dropped = await bob.waitEvent('producerReceiving', (data) => data.producerId === screen.answer.producerId && data.receiving === false, 15000).catch(() => null);
    ok('(a) producerReceiving=false after the publisher stops sending (contract: ~1.5 s)', Boolean(dropped),
        dropped ? `after ${dropped.at - stoppedAt} ms` : 'never emitted in 15 s');
    const late = await bob.call('consumePlain', { producerId: screen.answer.producerId, srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: watched.receiver.key } });
    ok('(a) a fresh consumePlain 15 s after the publisher went silent says receiving=false', late.data?.receiving === false, `receiving=${late.data?.receiving}`);

    // (b) mic that keeps sending silence (as the native app does when muted) but is paused
    const MIC = 0x22220002;
    const mic = await publish(alice, 'mic', MIC, screen.key, screen.sender);
    mic.sender.micTimer = setInterval(() => mic.sender.sendOpus(MIC), 20);
    await bob.waitEvent('producerReceiving', (data) => data.producerId === mic.answer.producerId && data.receiving, 5000);
    await alice.ok('pauseProducer', { producerId: mic.answer.producerId }); // self-mute
    const heard = await watch(bob, mic.answer.producerId, watched.receiver);
    log('consumePlain(mic) while the producer is paused ->', JSON.stringify({ receiving: heard.answer.receiving }));
    const before = watched.receiver.of(heard.answer.ssrc).length;
    await sleep(6000);
    const after = watched.receiver.of(heard.answer.ssrc).length;
    const flipped = bob.eventsNamed('producerReceiving').filter((entry) => entry.data.producerId === mic.answer.producerId && entry.data.receiving === false);
    ok('(b) paused producer: watcher gets 0 packets for 6 s (expected)', after - before === 0, `${after - before} packets`);
    ok('(b) ...but the SFU says receiving=true and never flips it (native ArrivalWatch then rebuilds the path every 10-60 s)',
        !(heard.answer.receiving === true && flipped.length === 0), `consumePlain.receiving=${heard.answer.receiving}, receiving=false events=${flipped.length}`);

    clearInterval(screen.sender.timer);
    clearInterval(mic.sender.micTimer);
    screen.sender.close();
    watched.receiver.close();
    alice.close();
    bob.close();
} finally {
    await sfu.stop();
}
console.log(failures ? `\n${failures} check(s) failed (FAIL = bug reproduced)` : '\nall checks passed');
process.exit(failures ? 1 : 0);
