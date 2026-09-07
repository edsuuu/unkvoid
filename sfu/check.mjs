import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';

const URL_WS = process.env.SFU_CHECK_URL ?? 'ws://127.0.0.1:3000/sfu';
const SECRET = process.env.SFU_SECRET ?? '';

assert.ok(SECRET, 'set SFU_SECRET to run the check');

const b64 = input => Buffer.from(typeof input === 'string' ? input : JSON.stringify(input))
    .toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

const mint = (claims, secret = SECRET) => {
    const body = { exp: Math.floor(Date.now() / 1000) + 600, ...claims };
    const input = `${b64({ alg: 'HS256', typ: 'JWT' })}.${b64(body)}`;
    const signature = createHmac('sha256', secret).update(input).digest('base64')
        .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

    return `${input}.${signature}`;
};

class Client {
    constructor() {
        this.socket = new WebSocket(URL_WS);
        this.pending = new Map();
        this.nextId = 1;
        this.events = [];
        this.socket.onmessage = message => {
            const payload = JSON.parse(message.data);

            if (payload.event) {
                this.events.push(payload);

                return;
            }

            this.pending.get(payload.id)?.(payload);
            this.pending.delete(payload.id);
        };
    }

    open() {
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => reject(new Error(`failed to open the socket at ${URL_WS} within 5s`)), 5000);

            this.socket.onopen = () => {
                clearTimeout(timer);
                resolve();
            };
            this.socket.onerror = () => {
                clearTimeout(timer);
                reject(new Error(`failed to connect to ${URL_WS}`));
            };
        });
    }

    call(action, data = {}) {
        const id = this.nextId++;

        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => reject(new Error(`no response for "${action}" within 5s`)), 5000);

            this.pending.set(id, reply => {
                clearTimeout(timer);
                resolve(reply);
            });
            this.socket.send(JSON.stringify({ id, action, data }));
        });
    }

    close() {
        this.socket.close();
    }
}

const run = async () => {
    const room = 'check-room';

    const guest = new Client();
    await guest.open();

    let reply = await guest.call('join', {});
    assert.equal(reply.ok, false, 'join without a token should fail');
    assert.equal(reply.status, 422, 'join without a token is a validation error');

    reply = await guest.call('join', { token: mint({ sub: 'u1', room, role: 'owner' }, 'segredo-errado') });
    assert.equal(reply.status, 401, 'wrong signature should return 401');

    reply = await guest.call('join', { token: mint({ sub: 'u1', room, role: 'owner', exp: 1 }) });
    assert.equal(reply.status, 401, 'expired token should return 401');

    reply = await guest.call('createTransport', {});
    assert.equal(reply.status, 401, 'action without a session should return 401');

    let owner = new Client();
    await owner.open();
    reply = await owner.call('join', { token: mint({ sub: 'owner-uuid', name: 'Owner', room, role: 'owner' }) });
    assert.equal(reply.ok, true, 'valid join should pass');
    assert.equal(reply.data.role, 'owner');
    assert.ok(reply.data.routerRtpCapabilities.codecs.length > 0, 'should return the router capabilities');

    reply = await owner.call('join', { token: mint({ sub: 'owner-uuid', room, role: 'owner' }) });
    assert.equal(reply.ok, false, 'cannot join twice on the same socket');

    const reconectado = new Client();
    await reconectado.open();
    reply = await reconectado.call('join', { token: mint({ sub: 'owner-uuid', name: 'Owner', room, role: 'owner' }) });
    assert.equal(reply.ok, true, 'the same person on another socket joins and replaces the old session');
    owner.close();
    owner = reconectado;

    reply = await owner.call('pauseConsumer', { consumerId: 'nao-existe' });
    assert.equal(reply.status, 404, 'pausing a missing consumer should return 404');

    reply = await owner.call('acaoQueNaoExiste', {});
    assert.equal(reply.status, 404, 'unknown action should return 404');

    reply = await owner.call('createTransport', {});
    assert.equal(reply.ok, true, 'createTransport deve passar');
    assert.ok(reply.data.iceCandidates.some(c => c.protocol === 'udp'), 'precisa anunciar candidato UDP');

    const member = new Client();
    await member.open();
    await member.call('join', { token: mint({ sub: 'member-uuid', name: 'Membro', room, role: 'member' }) });

    reply = await member.call('disconnectPeer', { peerId: 'owner-uuid' });
    assert.equal(reply.status, 403, 'regular members cannot disconnect anyone');

    reply = await owner.call('disconnectPeer', { peerId: 'owner-uuid' });
    assert.equal(reply.status, 403, 'nobody can moderate themselves');

    reply = await owner.call('stopBroadcast', { peerId: 'member-uuid' });
    assert.equal(reply.ok, true, 'owner can stop a member’s broadcast');

    reply = await owner.call('disconnectPeer', { peerId: 'nao-existe' });
    assert.equal(reply.status, 404, 'alvo inexistente deve dar 404');

    reply = await owner.call('disconnectPeer', { peerId: 'member-uuid' });
    assert.equal(reply.ok, true, 'owner can disconnect a call member');

    // A signaling drop must not remove the participant from the room: reconnecting within the grace period
    // should resume the session with media intact.
    const solucador = new Client();
    await solucador.open();
    reply = await solucador.call('join', { token: mint({ sub: 'soluco-uuid', name: 'Soluço', room, role: 'member' }) });
    assert.equal(reply.ok, true, 'normal entry before the drop test');
    assert.equal(reply.data.resumed, false, 'first join is not resumed');

    reply = await solucador.call('createTransport', {});
    const transportAntes = reply.data.transportId;
    assert.ok(transportAntes, 'transport created before the drop');

    solucador.socket.close();
    await new Promise(resolve => setTimeout(resolve, 800));

    const voltou = new Client();
    await voltou.open();
    reply = await voltou.call('join', { token: mint({ sub: 'soluco-uuid', name: 'Soluço', room, role: 'member' }), resume: true });
    assert.equal(reply.ok, true, 'reconnection within the grace period should join');
    assert.equal(reply.data.resumed, true, 'resume:true should RESUME the session');

    reply = await voltou.call('connectTransport', { transportId: transportAntes, dtlsParameters: { fingerprints: [], role: 'client' } });
    assert.notEqual(reply.status, 404, 'the transport from before the drop should still exist');

    // The P2P relay delivers from one participant to another, and the sender comes from the session:
    // no one can impersonate another person.
    const alice = new Client();
    await alice.open();
    await alice.call('join', { token: mint({ sub: 'alice-uuid', name: 'Alice', room, role: 'member' }) });

    const bob = new Client();
    await bob.open();
    await bob.call('join', { token: mint({ sub: 'bob-uuid', name: 'Bob', room, role: 'member' }) });

    bob.events.length = 0;
    reply = await alice.call('signal', { to: 'bob-uuid', kind: 'offer', payload: { sdp: 'v=0' } });
    assert.equal(reply.ok, true, 'sinal deve ser entregue');
    await new Promise(resolve => setTimeout(resolve, 400));

    const sinal = bob.events.find(evento => evento.event === 'signal');
    assert.ok(sinal, 'the recipient should receive the signal');
    assert.equal(sinal.data.from, 'alice-uuid', 'the sender comes from the session, not the body');
    assert.equal(sinal.data.kind, 'offer');
    assert.equal(sinal.data.payload.sdp, 'v=0');

    reply = await alice.call('signal', { to: 'nao-existe', kind: 'offer', payload: {} });
    assert.equal(reply.status, 404, 'signaling to someone outside the room should return 404');

    reply = await alice.call('signal', { to: 'bob-uuid', kind: 'invalido', payload: {} });
    assert.equal(reply.status, 422, 'unknown signal type should be rejected');

    alice.close();
    bob.close();

    // Viewers must be notified immediately when the broadcaster’s connection drops,
    // otherwise the last frame remains frozen and looks stuck.
    const espectador = new Client();
    await espectador.open();
    await espectador.call('join', { token: mint({ sub: 'espectador-uuid', name: 'Espectador', room, role: 'member' }) });

    const outro = new Client();
    await outro.open();
    await outro.call('join', { token: mint({ sub: 'quedavel-uuid', name: 'Quedável', room, role: 'member' }) });

    espectador.events.length = 0;
    outro.socket.close();
    await new Promise(resolve => setTimeout(resolve, 900));

    const avisoDeQueda = espectador.events.find(evento => evento.event === 'peerConnectionLost');
    assert.ok(avisoDeQueda, 'the room should be notified when someone’s signaling drops');
    assert.equal(avisoDeQueda.data.peerId, 'quedavel-uuid');

    const devolta = new Client();
    await devolta.open();
    espectador.events.length = 0;
    await devolta.call('join', { token: mint({ sub: 'quedavel-uuid', name: 'Quedável', room, role: 'member' }), resume: true });
    await new Promise(resolve => setTimeout(resolve, 600));

    assert.ok(
        espectador.events.some(evento => evento.event === 'peerReconnected'),
        'the room should be notified when the person returns',
    );

    devolta.close();
    espectador.close();

    // Without requesting a resume (F5 case: new client, no transports), start a clean session.
    voltou.socket.close();
    await new Promise(resolve => setTimeout(resolve, 600));

    const depoisDoF5 = new Client();
    await depoisDoF5.open();
    reply = await depoisDoF5.call('join', { token: mint({ sub: 'soluco-uuid', name: 'Soluço', room, role: 'member' }) });
    assert.equal(reply.ok, true, 'joining without requesting a resume should work');
    assert.equal(reply.data.resumed, false, 'without resume:true it MUST NOT resume — the client has no transports');

    depoisDoF5.close();
    voltou.close();
    guest.close();
    owner.close();
    member.close();

    console.log('SFU API check: OK');
};

run().catch(error => {
    console.error('check FAILED:', error.message);
    process.exit(1);
});
