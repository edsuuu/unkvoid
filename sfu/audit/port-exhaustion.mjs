// T04 — one guest socket, no account, exhausts the worker's plain-RTP port pool by firing
// concurrent consumePlain with distinct keys (each wins the `existing` check race and leaks a
// transport that lives until the peer leaves). Everybody else on that worker then fails
// producePlain/consumePlain with "limite de participantes".
import { readFileSync } from 'node:fs';

import { check, joinGuest, log, newKey, producePlainBody, sleep, startSfu } from './lib.mjs';

const PORTS = 8;
const sfu = await startSfu({ base: 3405, workers: 1, plainPorts: PORTS });
let failures = 0;
const ok = (...args) => check(...args) || failures++;
const listening = () => {
    const rows = readFileSync('/proc/net/udp', 'utf8').split('\n').slice(1).filter(Boolean);
    return rows.filter((row) => {
        const port = parseInt(row.trim().split(/\s+/)[1].split(':')[1], 16);
        return port >= 3405 + 20000 && port < 3405 + 20000 + PORTS;
    }).length;
};

try {
    const attacker = await joinGuest(sfu, 'attacker-room', 'mallory');
    const own = await attacker.ok('producePlain', producePlainBody('mic', 0x44440001, newKey()));
    const answers = await Promise.all([...Array(PORTS)].map(() =>
        attacker.call('consumePlain', { producerId: own.producerId, srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: newKey() } })));
    await sleep(200);
    log(`attacker: ${answers.filter((answer) => answer.ok).length} consumePlain ok, ${answers.filter((answer) => !answer.ok).length} failed; plain ports bound now: ${listening()}/${PORTS}`);

    const victim = await joinGuest(sfu, 'victim-room', 'victor');
    const shared = await victim.call('producePlain', producePlainBody('screen', 0x44440002, newKey()));
    ok('a different room can still publish its screen', shared.ok, shared.ok ? '' : `${shared.status} ${shared.error}`);

    attacker.close();
    await sleep(300);
    const retry = await victim.call('producePlain', producePlainBody('screen', 0x44440003, newKey()));
    log(`after the attacker leaves: producePlain ${retry.ok ? 'ok' : `${retry.status} ${retry.error}`} (ports bound: ${listening()})`);
    victim.close();
} finally {
    await sfu.stop();
}
console.log(failures ? `\n${failures} check(s) failed (FAIL = bug reproduced)` : '\nall checks passed');
process.exit(failures ? 1 : 0);
