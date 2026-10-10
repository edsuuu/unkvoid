import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';

import { Laravel } from '../lib/Laravel.mjs';
import { Participant } from '../lib/Participant.mjs';
import { cleanVideo, ports, record, sleep, waitFor } from '../lib/scenario.mjs';
import { SfuProcess } from '../lib/SfuProcess.mjs';
import { SignalClient } from '../lib/SignalClient.mjs';

const SCREEN = { width: 1280, height: 720, fps: 30, bitrate: 3_000_000 };
const CAMERA = { width: 640, height: 360, fps: 30, bitrate: 800_000 };

const unavailable = Laravel.unavailable();

let sfu;
let laravel;
let observer;
const people = [];
const accounts = {};
const realtime = [];
let origin;
let destination;

before(async () => {
    if (unavailable) {
        return;
    }

    const slot = ports(4);

    sfu = new SfuProcess({ ...slot, workers: 2, env: { SFU_LARAVEL_URL: `http://127.0.0.1:${slot.laravel}` } });
    laravel = new Laravel({ port: slot.laravel, sfu });
    await sfu.start();
    await laravel.start();

    for (const nickname of ['dono', 'ana', 'bia', 'caio', 'davi']) {
        accounts[nickname] = await laravel.register(nickname);
    }

    const server = (await laravel.api('POST', '/servers', { name: 'Servidor E2E' }, accounts.dono.token)).body;
    const second = await laravel.api('POST', `/servers/${server.id}/channels`, { name: 'Sala 2', type: 'voice' }, accounts.dono.token);
    const tree = (await laravel.api('GET', `/servers/${server.id}`, undefined, accounts.dono.token)).body;
    const invite = (await laravel.api('POST', `/servers/${server.id}/invite`, {}, accounts.dono.token)).body.invite_code;

    origin = tree.channels.find(channel => channel.type === 'voice' && channel.id !== second.body.id).id;
    destination = second.body.id;

    for (const nickname of ['ana', 'bia', 'caio', 'davi']) {
        const joined = await laravel.api('POST', `/invites/${invite}`, {}, accounts[nickname].token);

        assert.ok(joined.status < 300, `invite ${nickname}: ${joined.status}`);
    }

    // O dono ouve o tempo real dos dois canais, como o app que mostra quem está em cada voz.
    const session = (await laravel.api('POST', '/sfu/session', {}, accounts.dono.token)).body;

    observer = await new SignalClient(sfu.url).open();
    observer.onEvent((event, data) => realtime.push({ at: Date.now(), event, data }));
    await observer.call('identify', { token: session.token });
    await observer.call('subscribe', { channel: `channel.${origin}` });
    await observer.call('subscribe', { channel: `channel.${destination}` });
});

after(async () => {
    for (const person of people) {
        person.crash();
    }

    observer?.close();
    await laravel?.stop();
    await sfu?.stop();
});

const member = async (nickname, channel, options = {}) => {
    const person = new Participant({
        name: nickname,
        url: sfu.url,
        identity: laravel.voiceIdentity(accounts[nickname], channel),
        moveIdentity: to => laravel.voiceIdentity(accounts[nickname], to)(),
        gate: 'immediate',
        ...options,
    });

    people.push(person);

    return person.join();
};

const roomOf = (presence, nickname) => Object.entries(presence.rooms).filter(([, peers]) => peers.some(peer => peer.sub === `user:${accounts[nickname].user.id}`)).map(([room]) => room);

test('e. mover quem transmite de canal: a origem para de receber, o destino passa a receber, sem producer nem consumer fantasma', { skip: unavailable ?? false }, async () => {
    const bia = await member('bia', origin);
    const caio = await member('caio', destination);
    const ana = await member('ana', origin);

    await ana.publish('screen', SCREEN);
    await ana.publish('camera', CAMERA);
    await ana.publish('mic');
    await waitFor(() => bia.watchSummaries().filter(summary => summary.decodable > 0).length === 2, 5000, 'bia watching ana');
    // A lista de voz da origem já mostra a Ana: num Laravel lento, o `joined` dela chegava depois do mover.
    await waitFor(() => realtime.some(entry => entry.event === 'VoiceStateUpdated' && entry.data.user_id === accounts.ana.user.id && entry.data.channel_id === origin && entry.data.event === 'joined'), 15_000, 'the origin list showing ana');

    const before = { totals: await sfu.totals(), presence: await sfu.presence() };
    const watchedAtOrigin = [...bia.consumers.keys()];
    const movedAt = Date.now();
    const move = await laravel.api('PATCH', `/channels/${origin}/voice/members/${accounts.ana.user.id}`, { channel_id: destination }, accounts.dono.token);

    assert.equal(move.status, 204, `PATCH move: ${move.status} ${JSON.stringify(move.body)}`);

    const moved = await waitFor(() => ana.events('moved')[0], 3000, 'ana receiving moved');

    await waitFor(() => watchedAtOrigin.every(producerId => !bia.consumers.has(producerId)), 3000, 'bia losing ana');

    const lostAtOrigin = Date.now() - movedAt;
    const arrivedAt = await waitFor(
        () => {
            const screen = caio.watchSummaries().find(summary => summary.label.endsWith(':screen'));

            return screen?.decodable > 0 ? Date.now() : null;
        },
        8000,
        'caio watching ana',
    );

    caio.resetWatches();
    await sleep(3000);

    const afterMove = { totals: await sfu.totals(), presence: await sfu.presence() };
    const binPacketsAfter = bia.receiver?.stats.datagrams ?? 0;

    await sleep(1000);

    const ghostPackets = (bia.receiver?.stats.datagrams ?? 0) - binPacketsAfter;
    const caioSees = caio.watchSummaries();
    const backToOrigin = await laravel.api('POST', `/channels/${origin}/voice/token`, {}, accounts.ana.token);
    const voiceEvents = realtime.filter(entry => entry.event === 'VoiceStateUpdated' && entry.data.user_id === accounts.ana.user.id && entry.at >= movedAt);

    record('e', {
        movedEvent: moved.data,
        lostAtOriginMs: lostAtOrigin,
        firstFrameAtDestinationMs: arrivedAt - movedAt,
        before,
        after: afterMove,
        caioSees,
        ghostPackets,
        backToOriginStatus: backToOrigin.status,
        voiceEvents: voiceEvents.map(entry => entry.data),
    });

    assert.deepEqual(moved.data, { to: destination, by: 'dono' });
    assert.deepEqual(roomOf(afterMove.presence, 'ana'), [destination]);
    assert.deepEqual(afterMove.presence.rooms[origin].map(peer => peer.name), ['bia']);
    // A Bia não assiste mais nada, e o que ainda pudesse chegar da Ana para no SFU.
    assert.equal(bia.consumers.size, 0);
    assert.equal(ghostPackets, 0, `${ghostPackets} pacotes da Ana ainda chegaram à Bia`);
    // Os 3 producers da Ana (agora no destino) e os 3 consumers do Caio, e mais nada.
    assert.deepEqual({ producers: afterMove.totals.producers, consumers: afterMove.totals.consumers }, { producers: 3, consumers: 3 });
    assert.deepEqual(caioSees.filter(summary => summary.label.endsWith(':screen')).flatMap(summary => cleanVideo(summary, SCREEN)), []);
    assert.ok(arrivedAt - movedAt <= 3000, `o destino viu a tela ${arrivedAt - movedAt} ms depois do PATCH`);
    assert.equal(backToOrigin.status, 403, 'a origem aceitou o token de quem acabou de ser movido');
    assert.deepEqual(
        voiceEvents.map(entry => [entry.data.channel_id === origin ? 'origem' : 'destino', entry.data.event]).sort(),
        [['destino', 'joined'], ['origem', 'left']],
    );
});

test('e. o app antigo, que não conhece `moved`, volta para a origem e é recusado: não reaparece lá', { skip: unavailable ?? false }, async () => {
    const davi = await member('davi', origin, { legacyMoved: true });

    await davi.publish('mic');
    await sleep(500);

    const move = await laravel.api('PATCH', `/channels/${origin}/voice/members/${accounts.davi.user.id}`, { channel_id: destination }, accounts.dono.token);

    assert.equal(move.status, 204);
    await waitFor(() => davi.joins.refused > 0, 10_000, 'the old app trying again');

    const presence = await sfu.presence();

    record('e-app-antigo', { refused: davi.joins.refused, state: davi.state, rooms: roomOf(presence, 'davi') });

    assert.deepEqual(roomOf(presence, 'davi'), []);
});
