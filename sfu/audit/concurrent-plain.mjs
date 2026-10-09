// T03 — two producePlain (or two consumePlain) on the same peer in flight at once, same key.
// Room.plainTransport() checks `existing` before an await, so both create a transport.
// The native app points ONE PlainSender at the last answer's port (sharing.rs use_sfu) and
// ONE PlainReceiver per server address (watching.rs start: other address => stop(None)).
import { check, joinGuest, log, newKey, producePlainBody, sleep, startSfu, PlainSender } from './lib.mjs';

const sfu = await startSfu({ base: 3404 });
let failures = 0;
const ok = (...args) => check(...args) || failures++;
const waitDead = process.argv.includes('--dead');

try {
    const alice = await joinGuest(sfu, 'concurrent', 'alice');
    const bob = await joinGuest(sfu, 'concurrent', 'bob');

    // Mic and screen opened at the same moment (two UI commands), same sender key.
    const key = newKey();
    const MIC = 0x33330001;
    const SCREEN = 0x33330002;
    const [mic, screen] = await Promise.all([
        alice.call('producePlain', producePlainBody('mic', MIC, key)),
        alice.call('producePlain', producePlainBody('screen', SCREEN, key)),
    ]);
    log('mic    ->', mic.data.port, ' screen ->', screen.data.port);
    ok('concurrent producePlain with the same key reuse ONE send transport', mic.data.port === screen.data.port,
        `ports ${mic.data.port} vs ${screen.data.port}`);

    // Do what the native app does: everything goes to the port of the last answer.
    const sender = await new PlainSender('alice').bind();
    sender.point(screen.data, key);
    const timer = setInterval(() => {
        sender.sendOpus(MIC);
        sender.frame(SCREEN, { gop: 30 });
    }, 33);
    const receiving = new Set();
    await sleep(3000);
    for (const entry of bob.eventsNamed('producerReceiving')) if (entry.data.receiving) receiving.add(entry.data.producerId);
    ok('both producers receive media when sent to the last port', receiving.has(mic.data.producerId) && receiving.has(screen.data.producerId),
        `mic receiving=${receiving.has(mic.data.producerId)} screen receiving=${receiving.has(screen.data.producerId)}`);

    if (waitDead) {
        const dead = await alice.waitEvent('producerDead', () => true, 32000).catch(() => null);
        ok('no producerDead for a producer the app is actually feeding', !dead, dead ? JSON.stringify(dead.data) : '');
    }

    // Watching: two consumePlain in flight at once with the same receive key.
    const rxKey = newKey();
    const srtp = { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: rxKey };
    const [a, b] = await Promise.all([
        bob.call("consumePlain", { producerId: screen.data.producerId, srtpParameters: srtp }),
        bob.call('consumePlain', { producerId: screen.data.producerId, srtpParameters: srtp }),
    ]);
    log('consume mic ->', a.data.port, ' consume screen ->', b.data.port);
    ok('concurrent consumePlain with the same key share ONE receive transport', a.data.port === b.data.port, `ports ${a.data.port} vs ${b.data.port}`);

    // Unbounded: 6 concurrent consumePlain with 6 different keys -> how many transports stay open?
    const answers = await Promise.all([...Array(6)].map(() =>
        bob.call("consumePlain", { producerId: screen.data.producerId, srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: newKey() } })));
    const ports = new Set(answers.filter((answer) => answer.ok).map((answer) => answer.data.port));
    log('6 concurrent rekeys ->', [...ports].join(','));
    ok('concurrent rekeys leave at most one receive transport', ports.size <= 1, `${ports.size} distinct ports still allocated`);

    clearInterval(timer);
    sender.close();
    alice.close();
    bob.close();
} finally {
    await sfu.stop();
}
console.log(failures ? `\n${failures} check(s) failed (FAIL = bug reproduced)` : '\nall checks passed');
process.exit(failures ? 1 : 0);
