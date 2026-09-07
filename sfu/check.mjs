import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';

const URL_WS = process.env.SFU_CHECK_URL ?? 'ws://127.0.0.1:3000/sfu';
const SECRET = process.env.SFU_SECRET ?? '';

assert.ok(SECRET, 'defina SFU_SECRET para rodar o check');

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
            const timer = setTimeout(() => reject(new Error(`não abriu o socket em ${URL_WS} em 5s`)), 5000);

            this.socket.onopen = () => {
                clearTimeout(timer);
                resolve();
            };
            this.socket.onerror = () => {
                clearTimeout(timer);
                reject(new Error(`não conectou em ${URL_WS}`));
            };
        });
    }

    call(action, data = {}) {
        const id = this.nextId++;

        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => reject(new Error(`sem resposta para "${action}" em 5s`)), 5000);

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
    assert.equal(reply.ok, false, 'join sem token deve falhar');
    assert.equal(reply.status, 422, 'join sem token é erro de validação');

    reply = await guest.call('join', { token: mint({ sub: 'u1', room, role: 'owner' }, 'segredo-errado') });
    assert.equal(reply.status, 401, 'assinatura errada deve dar 401');

    reply = await guest.call('join', { token: mint({ sub: 'u1', room, role: 'owner', exp: 1 }) });
    assert.equal(reply.status, 401, 'token expirado deve dar 401');

    reply = await guest.call('createTransport', {});
    assert.equal(reply.status, 401, 'ação sem sessão deve dar 401');

    let owner = new Client();
    await owner.open();
    reply = await owner.call('join', { token: mint({ sub: 'owner-uuid', name: 'Dono', room, role: 'owner' }) });
    assert.equal(reply.ok, true, 'join válido deve passar');
    assert.equal(reply.data.role, 'owner');
    assert.ok(reply.data.routerRtpCapabilities.codecs.length > 0, 'deve devolver as capabilities do router');

    reply = await owner.call('join', { token: mint({ sub: 'owner-uuid', room, role: 'owner' }) });
    assert.equal(reply.ok, false, 'não pode entrar duas vezes no mesmo socket');

    const reconectado = new Client();
    await reconectado.open();
    reply = await reconectado.call('join', { token: mint({ sub: 'owner-uuid', name: 'Dono', room, role: 'owner' }) });
    assert.equal(reply.ok, true, 'a mesma pessoa em outro socket entra e derruba a sessão antiga');
    owner.close();
    owner = reconectado;

    reply = await owner.call('pauseConsumer', { consumerId: 'nao-existe' });
    assert.equal(reply.status, 404, 'pausar consumer inexistente deve dar 404');

    reply = await owner.call('acaoQueNaoExiste', {});
    assert.equal(reply.status, 404, 'ação desconhecida deve dar 404');

    reply = await owner.call('createTransport', {});
    assert.equal(reply.ok, true, 'createTransport deve passar');
    assert.ok(reply.data.iceCandidates.some(c => c.protocol === 'udp'), 'precisa anunciar candidato UDP');

    const member = new Client();
    await member.open();
    await member.call('join', { token: mint({ sub: 'member-uuid', name: 'Membro', room, role: 'member' }) });

    reply = await member.call('disconnectPeer', { peerId: 'owner-uuid' });
    assert.equal(reply.status, 403, 'membro comum não pode desconectar ninguém');

    reply = await owner.call('disconnectPeer', { peerId: 'owner-uuid' });
    assert.equal(reply.status, 403, 'ninguém modera a si mesmo');

    reply = await owner.call('stopBroadcast', { peerId: 'member-uuid' });
    assert.equal(reply.ok, true, 'dono pode encerrar transmissão de membro');

    reply = await owner.call('disconnectPeer', { peerId: 'nao-existe' });
    assert.equal(reply.status, 404, 'alvo inexistente deve dar 404');

    reply = await owner.call('disconnectPeer', { peerId: 'member-uuid' });
    assert.equal(reply.ok, true, 'dono pode desconectar membro da chamada');

    // Queda de sinalização não pode derrubar da sala: reconectar dentro da carência
    // deve retomar a sessão, com a mídia intacta.
    const solucador = new Client();
    await solucador.open();
    reply = await solucador.call('join', { token: mint({ sub: 'soluco-uuid', name: 'Soluço', room, role: 'member' }) });
    assert.equal(reply.ok, true, 'entrada normal antes do teste de queda');
    assert.equal(reply.data.resumed, false, 'primeira entrada não é retomada');

    reply = await solucador.call('createTransport', {});
    const transportAntes = reply.data.transportId;
    assert.ok(transportAntes, 'transport criado antes da queda');

    solucador.socket.close();
    await new Promise(resolve => setTimeout(resolve, 800));

    const voltou = new Client();
    await voltou.open();
    reply = await voltou.call('join', { token: mint({ sub: 'soluco-uuid', name: 'Soluço', room, role: 'member' }) });
    assert.equal(reply.ok, true, 'reconexão dentro da carência deve entrar');
    assert.equal(reply.data.resumed, true, 'deve RETOMAR a sessão, não criar outra');

    reply = await voltou.call('connectTransport', { transportId: transportAntes, dtlsParameters: { fingerprints: [], role: 'client' } });
    assert.notEqual(reply.status, 404, 'o transport de antes da queda ainda deve existir');

    voltou.close();
    guest.close();
    owner.close();
    member.close();

    console.log('check da API do SFU: OK');
};

run().catch(error => {
    console.error('check FALHOU:', error.message);
    process.exit(1);
});
