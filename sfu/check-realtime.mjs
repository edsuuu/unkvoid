/**
 * O tempo real do SFU contra um servidor no ar: identificar o socket, inscrever com a
 * autorização do Laravel, receber o que o Laravel publica e sair da presença ao cair.
 *
 * Sobe um Laravel de mentira que confere a assinatura e responde quem pode ouvir o quê.
 *
 *   SFU_SECRET=<o mesmo do servidor> node --test check-realtime.mjs
 */
import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';
import { createServer } from 'node:http';
import { after, before, test } from 'node:test';

import { WebSocket as WsSocket } from 'ws';

const SECRET = process.env.SFU_SECRET ?? '';
const URL = process.env.SFU_CHECK_URL ?? 'ws://127.0.0.1:3000/sfu';
const HTTP = URL.replace(/^ws/, 'http').replace(/\/sfu$/, '');
const FAKE_LARAVEL_PORT = Number(process.env.FAKE_LARAVEL_PORT ?? 8099);

const hmac = (input) => createHmac('sha256', SECRET).update(input).digest('hex');

const token = (claims) => {
    const body = Buffer.from(JSON.stringify(claims)).toString('base64url');

    return `${body}.${hmac(body)}`;
};

const sessionToken = (sub, name) =>
    token({ sub, name, exp: Math.floor(Date.now() / 1000) + 60 });

const signedFetch = async (method, path, payload) => {
    const at = Math.floor(Date.now() / 1000);
    const body = payload === undefined ? '' : JSON.stringify(payload);

    return fetch(`${HTTP}${path}`, {
        method,
        ...(payload === undefined ? {} : { body }),
        headers: {
            'content-type': 'application/json',
            'x-unkvoid-timestamp': String(at),
            'x-unkvoid-signature': hmac(`${at}\n${method}\n${path}\n${body}`),
        },
    });
};

/** Diz sim para todo canal menos `channel.999`, que é o canal proibido dos testes. */
let laravel;
let authorizeCalls = [];

const startFakeLaravel = () =>
    new Promise((resolve) => {
        laravel = createServer((request, response) => {
            let raw = '';

            request.on('data', (chunk) => (raw += chunk));
            request.on('end', () => {
                const at = request.headers['x-unkvoid-timestamp'];
                const signature = request.headers['x-unkvoid-signature'];
                const expected = hmac(`${at}\nPOST\n${request.url}\n${raw}`);

                if (signature !== expected) {
                    response.writeHead(401).end('{}');

                    return;
                }

                const { sub, channel } = JSON.parse(raw);

                authorizeCalls.push({ sub, channel });

                response.writeHead(200, { 'content-type': 'application/json' });
                response.end(JSON.stringify({ allowed: channel !== 'channel.999', name: 'Nome do Laravel' }));
            });
        });

        laravel.listen(FAKE_LARAVEL_PORT, '127.0.0.1', resolve);
    });

const open = async () => {
    const socket = new WsSocket(URL);
    const inbox = [];
    const waiters = [];

    socket.on('message', (raw) => {
        const message = JSON.parse(raw.toString());

        inbox.push(message);

        for (const [index, waiter] of waiters.entries()) {
            if (waiter.matches(message)) {
                waiters.splice(index, 1);
                waiter.resolve(message);

                break;
            }
        }
    });

    await new Promise((resolve, reject) => {
        socket.once('open', resolve);
        socket.once('error', reject);
    });

    let nextId = 1;

    return {
        socket,
        inbox,
        call(action, data) {
            const id = nextId++;

            socket.send(JSON.stringify({ id, action, data }));

            return this.waitFor((message) => message.id === id);
        },
        waitFor(matches, timeout = 3000) {
            const found = inbox.find(matches);

            if (found) {
                return Promise.resolve(found);
            }

            return new Promise((resolve, reject) => {
                const timer = setTimeout(() => reject(new Error('timeout esperando mensagem')), timeout);

                waiters.push({
                    matches,
                    resolve: (message) => {
                        clearTimeout(timer);
                        resolve(message);
                    },
                });
            });
        },
        close() {
            return new Promise((resolve) => {
                socket.once('close', resolve);
                socket.close();
            });
        },
    };
};

before(async () => {
    assert.ok(SECRET, 'defina SFU_SECRET com o mesmo segredo do servidor');
    await startFakeLaravel();
});

after(() => laravel?.close());

test('sem identify, a inscrição é recusada', async () => {
    const client = await open();
    const answer = await client.call('subscribe', { channel: 'channel.1' });

    assert.equal(answer.ok, false);
    assert.equal(answer.status, 401);

    await client.close();
});

test('o identify exige um token assinado por quem tem o segredo', async () => {
    const client = await open();
    const answer = await client.call('identify', { token: 'lixo.invalido' });

    assert.equal(answer.ok, false);

    await client.close();
});

test('identificado, a inscrição pergunta ao Laravel e devolve a presença', async () => {
    authorizeCalls = [];

    const client = await open();

    assert.equal((await client.call('identify', { token: sessionToken('user:7', 'Ana') })).ok, true);

    const answer = await client.call('subscribe', { channel: 'channel.1' });

    assert.equal(answer.ok, true);
    assert.equal(answer.data.channel, 'channel.1');
    assert.deepEqual(authorizeCalls, [{ sub: 'user:7', channel: 'channel.1' }]);
    assert.ok(answer.data.members.some((member) => member.id === 'user:7'));

    await client.close();
});

test('o canal de id ULID é aceito — é o formato real do canal de texto', async () => {
    const client = await open();
    const ulid = '01jbqz3h7k9m2n4p6r8t0v1w3x';

    await client.call('identify', { token: sessionToken('user:9', 'Ada') });

    const answer = await client.call('subscribe', { channel: `channel.${ulid}` });

    assert.equal(answer.ok, true, 'o ULID foi recusado pelo formato antes de chegar ao Laravel');
    assert.equal(answer.data.channel, `channel.${ulid}`);

    await client.close();
});

test('canal com formato inválido é recusado', async () => {
    const client = await open();

    await client.call('identify', { token: sessionToken('user:9', 'Ada') });

    for (const channel of ['channel.', 'outro.5', 'channel.com/barra', 'channel.' + 'x'.repeat(40)]) {
        const answer = await client.call('subscribe', { channel });

        assert.equal(answer.ok, false, `aceitou canal inválido: ${channel}`);
    }

    await client.close();
});

test('o canal que o Laravel recusa não entra', async () => {
    const client = await open();

    await client.call('identify', { token: sessionToken('user:8', 'Bia') });

    const answer = await client.call('subscribe', { channel: 'channel.999' });

    assert.equal(answer.ok, false);
    assert.equal(answer.status, 403);

    await client.close();
});

test('o que o Laravel publica chega a quem está inscrito, e só a ele', async () => {
    const dentro = await open();
    const fora = await open();

    await dentro.call('identify', { token: sessionToken('user:10', 'Ana') });
    await dentro.call('subscribe', { channel: 'channel.5' });

    await fora.call('identify', { token: sessionToken('user:11', 'Bia') });
    await fora.call('subscribe', { channel: 'channel.6' });

    const response = await signedFetch('POST', '/broadcast', {
        channel: 'channel.5',
        event: 'MessageSent',
        data: { id: 42, body: 'oi' },
    });

    assert.equal(response.status, 200);
    assert.equal((await response.json()).delivered, 1);

    const message = await dentro.waitFor((entry) => entry.event === 'MessageSent');

    assert.equal(message.channel, 'channel.5');
    assert.equal(message.data.body, 'oi');
    assert.ok(!fora.inbox.some((entry) => entry.event === 'MessageSent'));

    await dentro.close();
    await fora.close();
});

test('publicar sem assinatura não entrega nada', async () => {
    const response = await fetch(`${HTTP}/broadcast`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ channel: 'channel.5', event: 'MessageSent', data: {} }),
    });

    assert.equal(response.ok, false);
    assert.equal(response.status, 401);
});

test('quem já está no canal vê quem chega e quem sai', async () => {
    const primeiro = await open();
    const segundo = await open();

    await primeiro.call('identify', { token: sessionToken('user:20', 'Ana') });
    await primeiro.call('subscribe', { channel: 'channel.7' });

    await segundo.call('identify', { token: sessionToken('user:21', 'Bia') });
    await segundo.call('subscribe', { channel: 'channel.7' });

    const entrou = await primeiro.waitFor((entry) => entry.event === 'presence.joining');

    assert.equal(entrou.data.id, 'user:21');

    await segundo.close();

    const saiu = await primeiro.waitFor((entry) => entry.event === 'presence.leaving');

    assert.equal(saiu.data.id, 'user:21');

    await primeiro.close();
});

test('a mesma conta em duas máquinas conta como uma só na presença', async () => {
    const observador = await open();
    const primeira = await open();
    const segunda = await open();

    await observador.call('identify', { token: sessionToken('user:30', 'Ana') });
    await observador.call('subscribe', { channel: 'channel.8' });

    await primeira.call('identify', { token: sessionToken('user:31', 'Bia') });
    await primeira.call('subscribe', { channel: 'channel.8' });

    await segunda.call('identify', { token: sessionToken('user:31', 'Bia') });

    const answer = await segunda.call('subscribe', { channel: 'channel.8' });
    const vezes = answer.data.members.filter((member) => member.id === 'user:31').length;

    assert.equal(vezes, 1, 'a mesma conta apareceu duas vezes na presença');

    // A primeira máquina saindo não pode anunciar que a pessoa saiu: ela continua na outra.
    await primeira.close();
    await new Promise((resolve) => setTimeout(resolve, 300));

    assert.ok(
        !observador.inbox.some(
            (entry) => entry.event === 'presence.leaving' && entry.data.id === 'user:31',
        ),
        'anunciou saída de quem ainda estava conectado pela outra máquina',
    );

    await segunda.close();

    const saiu = await observador.waitFor(
        (entry) => entry.event === 'presence.leaving' && entry.data.id === 'user:31',
    );

    assert.equal(saiu.data.id, 'user:31');

    await observador.close();
});
