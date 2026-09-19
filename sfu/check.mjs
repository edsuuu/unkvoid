/**
 * O contrato do SFU, conferido contra um servidor de verdade.
 *
 * Sobe o SFU (`npm run build && SFU_CONNECTIONS_PER_MINUTE=200 node dist/server.js`) e
 * rode `npm run check`. O teto de conexões precisa ser levantado porque a conferência
 * abre uma dúzia de clientes de uma vez, que é exatamente o que ele existe para barrar.
 *
 * Dois cenários, o do webhook e o do heartbeat, sobem um SFU próprio a partir do `dist/`:
 * precisam de uma configuração que o servidor de todos não pode ter.
 */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { createServer } from 'node:http';
import { after, before, test } from 'node:test';

// O WebSocket do próprio Node responde ao ping sozinho e não tem como desligar: fingir um
// socket mudo, no cenário do heartbeat, só com o cliente do `ws`.
import { WebSocket as WsSocket } from 'ws';

const URL_WS = process.env.SFU_CHECK_URL ?? 'ws://127.0.0.1:3000/sfu';
const URL_HTTP = URL_WS.replace(/^ws/, 'http').replace(/\/sfu$/, '');
const SECRET = process.env.SFU_SECRET ?? 'segredo-de-teste-com-mais-de-32-caracteres';

const hmac = input => createHmac('sha256', SECRET).update(input).digest('hex');

/** O que o Laravel faz: assina quem entra, com que nome, e o que pode produzir. */
const token = (claims, secret = SECRET) => {
    const body = Buffer.from(JSON.stringify({ exp: Math.floor(Date.now() / 1000) + 60, ...claims })).toString('base64url');

    return `${body}.${createHmac('sha256', secret).update(body).digest('hex')}`;
};

/** E o cabeçalho com que ele chama o SFU direto. */
const signed = (method, path, body, timestamp = String(Math.floor(Date.now() / 1000))) => ({
    'content-type': 'application/json',
    'x-unkvoid-timestamp': timestamp,
    'x-unkvoid-signature': hmac(`${timestamp}\n${method}\n${path}\n${body}`),
});

/** O mínimo que um espectador precisa declarar para receber H.264 do app nativo. */
const CAPACIDADES = {
    codecs: [{
        kind: 'video',
        mimeType: 'video/H264',
        clockRate: 90000,
        preferredPayloadType: 96,
        parameters: { 'packetization-mode': 1, 'level-asymmetry-allowed': 1, 'profile-level-id': '42e01f' },
        rtcpFeedback: [{ type: 'nack' }, { type: 'nack', parameter: 'pli' }],
    }],
    headerExtensions: [],
};

const TUDO = ['speak', 'stream', 'video'];

const espera = ms => new Promise(resolve => setTimeout(resolve, ms));

/** Chave SRTP e parâmetros de um `producePlain` de áudio, para não repetir o bloco. */
const audioPuro = (source, ssrc) => ({
    kind: 'audio',
    source,
    srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: Buffer.alloc(30, 7).toString('base64') },
    rtpParameters: {
        codecs: [{ mimeType: 'audio/opus', payloadType: 111, clockRate: 48000, channels: 2, rtcpFeedback: [] }],
        encodings: [{ ssrc }],
    },
});

/** O mesmo, para vídeo H.264 — a tela e a câmera do app nativo. */
const videoPuro = (source, ssrc) => ({
    kind: 'video',
    source,
    srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: Buffer.alloc(30, 7).toString('base64') },
    rtpParameters: {
        codecs: [{
            mimeType: 'video/H264',
            payloadType: 96,
            clockRate: 90000,
            parameters: { 'packetization-mode': 1, 'level-asymmetry-allowed': 1, 'profile-level-id': '42e01f' },
            rtcpFeedback: [{ type: 'nack' }, { type: 'nack', parameter: 'pli' }],
        }],
        encodings: [{ ssrc }],
    },
});

class Client {
    constructor(url = URL_WS) {
        this.socket = new WebSocket(url);
        this.pending = new Map();
        this.nextId = 1;
        this.events = [];
        this.closeCode = null;
        this.socket.onclose = event => (this.closeCode = event.code);
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

/** Entra na sala e devolve a resposta já conferida. */
const entrar = async (cliente, data) => {
    const reply = await cliente.call('join', data);

    assert.equal(reply.ok, true, `join deveria passar: ${JSON.stringify(reply)}`);

    return reply.data;
};

/** Todo cliente aberto aqui: sem fechar no fim, os sockets seguram o processo. */
const abertos = [];

const abrir = async () => {
    const cliente = new Client();

    await cliente.open();
    abertos.push(cliente);

    return cliente;
};

const room = 'checkroom001';

/** O que um cenário deixa para o seguinte: a ordem dos testes é a da chamada de verdade. */
let visitante;
let dono;
let entrada;
let soluco;
let nativo;
let plain;
let assistindo;
let consumo;
let plainAudio;
let verVideo;
let chaveVer;
let plateia;
let retomando;
let sessao;
let micDaSessao;
let telaDaSessao;

/** Derruba a sinalização e volta dentro da carência, como o app: token novo, a mesma chave. */
const retomar = async (cliente, data) => {
    cliente.socket.close();
    await espera(800);

    const volta = await abrir();

    return { volta, retomada: await entrar(volta, { ...data, resume: true }) };
};

const salaRetomada = 'checkroom003';
const tokenRetomada = can => token({ room: salaRetomada, sub: '81', name: 'Volta', can });

before(async () => {
    visitante = await abrir();
});

after(() => {
    for (const cliente of abertos) {
        cliente.close();
    }
});

test('o join só aceita token bem formado, assinado e no prazo', async () => {
    let reply = await visitante.call('join', {});
    assert.equal(reply.status, 422, 'entrar sem token é erro de validação');

    reply = await visitante.call('join', { token: 'lixo' });
    assert.equal(reply.status, 422, 'token sem o formato corpo.assinatura é erro de validação');

    reply = await visitante.call('join', { token: token({ room, sub: '1', name: 'X', can: TUDO }, 'outro-segredo') });
    assert.equal(reply.status, 401, 'token assinado com outro segredo tem de ser recusado');

    reply = await visitante.call('join', { token: token({ room, sub: '1', name: 'X', can: TUDO, exp: 1 }) });
    assert.equal(reply.status, 401, 'token vencido tem de ser recusado');

    reply = await visitante.call('join', { token: token({ room, sub: '1', name: 'X' }) });
    assert.equal(reply.status, 422, 'token sem `can` está incompleto');

    reply = await visitante.call('createTransport', {});
    assert.equal(reply.status, 401, 'ação sem sessão deve dar 401');
});

test('o join antigo entra sem token, mas recusa sala de 26 caracteres', async () => {
    const reply = await visitante.call('join', { room: '01ARZ3NDEKTSV4RRFFQ69G5FAV', name: 'Intruso' });
    assert.equal(reply.status, 422, 'canal de servidor (ULID) sem token tem de ser recusado, mesmo no join antigo');

    // O app de hoje ainda entra sem token, como visitante. Some quando todos souberem pedir um.
    const antigo = await abrir();
    const legado = await entrar(antigo, { room, name: 'Legado', installId: 'inst-1' });
    assert.deepEqual(legado.can, TUDO, 'sem token entra com tudo liberado');
    assert.equal(legado.userId, 'guest:inst-1', 'e a identidade é a instalação');
    antigo.close();
});

test('a identidade nasce no servidor e o mesmo socket não entra duas vezes', async () => {
    // A identidade nasce no servidor: ninguém escolhe o próprio id.
    dono = await abrir();
    entrada = await entrar(dono, { token: token({ room, sub: '10', name: 'Dono', can: ['speak'] }) });

    assert.deepEqual(entrada.can, ['speak'], 'o que o Laravel liberou volta na resposta');
    assert.equal(entrada.userId, '10', 'e sabe qual conta é');

    assert.ok(entrada.peerId, 'o servidor devolve o id do participante');
    assert.ok(entrada.resumeKey, 'e a chave para voltar depois de uma queda');
    assert.notEqual(entrada.peerId, entrada.resumeKey, 'a chave não pode ser o id, que a sala inteira conhece');
    assert.ok(entrada.routerRtpCapabilities.codecs.length > 0, 'e as capacidades do router');
    assert.equal(entrada.resumed, false, 'a primeira entrada não é retomada');

    let reply = await dono.call('join', { token: token({ room, sub: '10', name: 'Dono', can: ['speak'] }) });
    assert.equal(reply.ok, false, 'não dá para entrar duas vezes no mesmo socket');

    reply = await dono.call('pauseConsumer', { consumerId: 'nao-existe' });
    assert.equal(reply.status, 404, 'pausar consumer inexistente deve dar 404');

    reply = await dono.call('acaoQueNaoExiste', {});
    assert.equal(reply.status, 404, 'ação desconhecida deve dar 404');
});

test('createTransport passa e produce recusa transporte e parâmetros inválidos', async () => {
    let reply = await dono.call('createTransport', {});
    assert.equal(reply.ok, true, 'createTransport deve passar');
    assert.ok(reply.data.iceCandidates.some(c => c.protocol === 'udp'), 'precisa anunciar candidato UDP');

    // `produce` de verdade precisa de um sendTransport do mediasoup-client; aqui só a
    // validação e a permissão, que são o que o servidor decide sozinho.
    const rtp = { codecs: [], encodings: [] };
    const transporteDono = reply.data.transportId;

    reply = await dono.call('produce', { kind: 'audio', source: 'mic', rtpParameters: rtp });
    assert.equal(reply.status, 422, 'produce sem transportId é erro de validação');

    reply = await dono.call('produce', { transportId: 'nao-existe', kind: 'audio', source: 'mic', rtpParameters: rtp });
    assert.equal(reply.status, 404, 'produce num transport desconhecido dá 404');

    reply = await dono.call('produce', { transportId: transporteDono, kind: 'audio', source: 'mic', rtpParameters: [] });
    assert.equal(reply.status, 422, 'um array no lugar do objeto é erro de validação, não 500');

    reply = await dono.call('produce', { transportId: transporteDono, kind: 'video', source: 'camera', rtpParameters: rtp });
    assert.equal(reply.status, 403, 'câmera sem `video` é recusada antes de tocar no transport');
});

test('produce e producePlain recusam origem sem a permissão do token', async () => {
    const rtp = { codecs: [], encodings: [] };

    const calado = await abrir();
    await entrar(calado, { token: token({ room, sub: '15', name: 'Calado', can: ['stream'] }) });

    let reply = await calado.call('produce', { transportId: 'x', kind: 'audio', source: 'mic', rtpParameters: rtp });
    assert.equal(reply.status, 403, 'mic sem `speak` é recusado');

    reply = await calado.call('producePlain', audioPuro('mic', 0x100));
    assert.equal(reply.status, 403, 'e por RTP puro também');

    const semTela = await abrir();
    await entrar(semTela, { token: token({ room, sub: '16', name: 'SemTela', can: ['speak'] }) });

    reply = await semTela.call('producePlain', { ...audioPuro('screenAudio', 0x101) });
    assert.equal(reply.status, 403, 'tela sem `stream` é recusada');

    calado.close();
    semTela.close();
});

test('o peerId de outro não retoma a sessão de ninguém', async () => {
    // Ninguém derruba ninguém sabendo o id alheio: sem a chave, é entrada nova.
    const impostor = await abrir();
    const outraSessao = await entrar(impostor, {
        token: token({ room, sub: '11', name: 'Impostor', can: TUDO }),
        resumeKey: entrada.peerId,
        resume: true,
    });

    assert.equal(outraSessao.resumed, false, 'o peerId de outro não retoma sessão nenhuma');
    assert.notEqual(outraSessao.peerId, entrada.peerId, 'e nem rouba o id');
    impostor.close();
});

test('a sala sabe da queda de sinalização e a retomada devolve a mídia intacta', async () => {
    // Queda de sinalização não tira ninguém da sala: voltar dentro da carência retoma
    // a sessão com a mídia intacta.
    const solucador = await abrir();
    soluco = await entrar(solucador, { token: token({ room, sub: '12', name: 'Soluço', can: TUDO }) });

    let reply = await solucador.call('createTransport', {});
    const transporteAntes = reply.data.transportId;
    assert.ok(transporteAntes, 'transporte criado antes da queda');

    dono.events.length = 0;
    solucador.socket.close();
    await espera(800);

    assert.ok(
        dono.events.some(evento => evento.event === 'peerConnectionLost' && evento.data.peerId === soluco.peerId),
        'a sala precisa saber na hora que a sinalização de alguém caiu',
    );

    const voltou = await abrir();
    dono.events.length = 0;
    const retomada = await entrar(voltou, {
        token: token({ room, sub: '12', name: 'Soluço', can: TUDO }),
        resumeKey: soluco.resumeKey,
        resume: true,
    });

    assert.equal(retomada.resumed, true, 'com a chave e resume:true a sessão é retomada');
    assert.equal(retomada.peerId, soluco.peerId, 'e é a mesma pessoa, com o mesmo id');

    reply = await voltou.call('connectTransport', { transportId: transporteAntes, dtlsParameters: { fingerprints: [], role: 'client' } });
    assert.notEqual(reply.status, 404, 'o transporte de antes da queda continua existindo');

    await espera(400);
    assert.ok(
        dono.events.some(evento => evento.event === 'peerReconnected'),
        'a sala precisa saber que a pessoa voltou',
    );

    // Sem pedir retomada (app reaberto, sem transporte nenhum), a sessão começa limpa.
    voltou.socket.close();
    await espera(600);
});

test('sem resume:true a sessão volta limpa, porque o cliente não tem transporte', async () => {
    const reaberto = await abrir();
    const limpa = await entrar(reaberto, {
        token: token({ room, sub: '12', name: 'Soluço', can: TUDO }),
        resumeKey: soluco.resumeKey,
    });

    assert.equal(limpa.resumed, false, 'sem resume:true NÃO pode retomar — o cliente não tem transporte');
    reaberto.close();
});

test('quem retoma recebe também quem está na carência, e quem chega não', async () => {
    // O app que retoma compara esta lista com a que já tinha: sem quem caiu junto, não
    // saberia dizer se a pessoa saiu da sala ou só está voltando também.
    const sala = 'checkroom002';
    const quemFica = await abrir();
    const ficou = await entrar(quemFica, { token: token({ room: sala, sub: '70', name: 'Fica', can: TUDO }) });

    let reply = await quemFica.call('producePlain', audioPuro('mic', 0x701));
    assert.equal(reply.ok, true, `o mic deveria subir: ${JSON.stringify(reply)}`);
    reply = await quemFica.call('pauseProducer', { producerId: reply.data.producerId });
    assert.equal(reply.ok, true, 'e pausar');

    const quemCai = await abrir();
    const caiu = await entrar(quemCai, { token: token({ room: sala, sub: '71', name: 'Cai', can: TUDO }) });

    quemFica.close();
    quemCai.close();
    await espera(800);

    const volta = await abrir();
    const retomada = await entrar(volta, {
        token: token({ room: sala, sub: '71', name: 'Cai', can: TUDO }),
        resumeKey: caiu.resumeKey,
        resume: true,
    });
    const fica = retomada.peers.find(peer => peer.peerId === ficou.peerId);

    assert.equal(retomada.resumed, true, 'a sessão é retomada');
    assert.equal(fica?.reconnecting, true, 'quem caiu junto vem na lista, marcado');
    assert.equal(fica.producers[0].paused, true, 'e o mic pausado vem pausado');

    const novo = await abrir();
    const chegada = await entrar(novo, { token: token({ room: sala, sub: '72', name: 'Novo', can: TUDO }) });

    assert.ok(!chegada.peers.some(peer => peer.peerId === ficou.peerId), 'quem chega não vê quem está na carência');
    assert.equal(chegada.peers.find(peer => peer.peerId === caiu.peerId)?.reconnecting, false, 'e vê quem voltou como presente');

    volta.close();
    novo.close();
});

test('a retomada com `can` reduzido fecha o producer que o token novo não cobre', async () => {
    // Quem decide permissão é o Laravel, e o token da reconexão é a palavra mais recente
    // dele: perder `stream` na carência tira a tela do ar na volta, e não na próxima entrada.
    plateia = await abrir();
    await entrar(plateia, { token: token({ room: salaRetomada, sub: '80', name: 'Plateia', can: TUDO }) });

    retomando = await abrir();
    sessao = await entrar(retomando, { token: tokenRetomada(TUDO) });

    micDaSessao = await retomando.call('producePlain', audioPuro('mic', 0x811));
    const tela = await retomando.call('producePlain', videoPuro('screen', 0x812));
    assert.equal(micDaSessao.ok && tela.ok, true, `mic e tela deveriam subir: ${JSON.stringify([micDaSessao, tela])}`);

    plateia.events.length = 0;
    const { volta, retomada } = await retomar(retomando, { token: tokenRetomada(['speak']), resumeKey: sessao.resumeKey });
    retomando = volta;
    await espera(300);

    assert.equal(retomada.resumed, true, 'perder permissão não impede a retomada');
    assert.equal(retomada.peerId, sessao.peerId, 'e é a mesma pessoa');
    assert.deepEqual(retomada.can, ['speak'], 'a resposta traz o `can` do token novo, e não o da entrada');

    const fechados = plateia.events.filter(evento => evento.event === 'producerClosed');
    assert.deepEqual(fechados.map(evento => evento.data.producerId), [tela.data.producerId], 'a sala vê a tela fechar, e só ela');
    assert.equal(fechados[0].data.source, 'screen', 'com a origem, como em qualquer `producerClosed`');
    const avisos = retomando.events.filter(evento => evento.event === 'producerDead');
    assert.equal(avisos.length, 1, 'a própria pessoa fica sabendo: o `producerClosed` não vai para o dono');
    assert.deepEqual(
        avisos[0].data,
        { producerId: tela.data.producerId, kind: 'video', source: 'screen', reason: 'revoked' },
        'com o motivo, que é o que separa a permissão perdida dos 30 s sem pacote',
    );

    let reply = await retomando.call('producePlain', videoPuro('screen', 0x813));
    assert.equal(reply.status, 403, 'sem `stream` a tela não volta');

    reply = await retomando.call('pauseProducer', { producerId: micDaSessao.data.producerId });
    assert.equal(reply.ok, true, 'o mic, que o token novo cobre, continua de pé');
});

test('a retomada com o mesmo `can` mantém tudo', async () => {
    plateia.events.length = 0;
    const { volta, retomada } = await retomar(retomando, { token: tokenRetomada(['speak']), resumeKey: sessao.resumeKey });
    retomando = volta;
    await espera(300);

    assert.equal(retomada.resumed, true, 'a sessão é retomada');
    assert.deepEqual(retomada.can, ['speak'], 'com o mesmo `can`');
    assert.ok(!plateia.events.some(evento => evento.event === 'producerClosed'), 'a sala não vê nada fechar');
    assert.ok(!retomando.events.some(evento => evento.event === 'producerDead'), 'nem a própria pessoa');

    const reply = await retomando.call('resumeProducer', { producerId: micDaSessao.data.producerId });
    assert.equal(reply.ok, true, 'e o mic de antes da queda segue obedecendo');
});

test('a retomada com `can` ampliado passa a permitir produzir', async () => {
    const { volta, retomada } = await retomar(retomando, { token: tokenRetomada(TUDO), resumeKey: sessao.resumeKey });
    retomando = volta;

    assert.equal(retomada.resumed, true, 'a sessão é retomada');
    assert.deepEqual(retomada.can, TUDO, 'a resposta traz o `can` ampliado');

    telaDaSessao = await retomando.call('producePlain', videoPuro('screen', 0x814));
    assert.equal(telaDaSessao.ok, true, `com stream de volta a tela sobe sem precisar entrar de novo: ${JSON.stringify(telaDaSessao)}`);
});

test('o mic revogado na retomada fecha para a sala, e o dono não recebe `producerDead`', async () => {
    // O app já instalado derruba a transmissão com qualquer `producerDead`, sem olhar a
    // origem: avisar do mic tiraria do ar a tela que o token novo continua permitindo.
    plateia.events.length = 0;
    const { volta, retomada } = await retomar(retomando, { token: tokenRetomada(['stream']), resumeKey: sessao.resumeKey });
    retomando = volta;
    await espera(300);

    assert.equal(retomada.resumed, true, 'a sessão é retomada');
    assert.deepEqual(retomada.can, ['stream'], 'e é pelo `can` da resposta que o app fica sabendo');

    const fechados = plateia.events.filter(evento => evento.event === 'producerClosed');
    assert.deepEqual(fechados.map(evento => evento.data.producerId), [micDaSessao.data.producerId], 'a sala vê o mic fechar, e só ele');
    assert.equal(fechados[0].data.source, 'mic', 'com a origem');
    assert.ok(!retomando.events.some(evento => evento.event === 'producerDead'), 'o dono não recebe `producerDead` do mic');

    let reply = await retomando.call('resumeProducer', { producerId: micDaSessao.data.producerId });
    assert.equal(reply.status, 404, 'o mic fechou de verdade, não ficou só pausado');

    reply = await retomando.call('pauseProducer', { producerId: telaDaSessao.data.producerId });
    assert.equal(reply.ok, true, 'e a tela, que o token novo cobre, continua de pé');
});

test('o visitante retoma sem token e nada muda', async () => {
    // Sem `installId` o `sub` do visitante é sorteado a cada entrada: a retomada dele não
    // pode depender de a identidade bater, que é o que vale para quem vem com token.
    const convidado = await abrir();
    const chegada = await entrar(convidado, { room: salaRetomada, name: 'Convidado' });
    const camera = await convidado.call('producePlain', videoPuro('camera', 0x821));
    assert.equal(camera.ok, true, `a câmera do visitante deveria subir: ${JSON.stringify(camera)}`);

    plateia.events.length = 0;
    const { volta, retomada } = await retomar(convidado, { room: salaRetomada, name: 'Convidado', resumeKey: chegada.resumeKey });
    await espera(300);

    assert.equal(retomada.resumed, true, 'o visitante retoma só com a chave');
    assert.equal(retomada.peerId, chegada.peerId, 'e é a mesma pessoa');
    assert.deepEqual(retomada.can, TUDO, 'com tudo liberado, como na entrada');
    assert.ok(!plateia.events.some(evento => evento.event === 'producerClosed'), 'a sala não vê nada fechar');

    const reply = await volta.call('pauseProducer', { producerId: camera.data.producerId });
    assert.equal(reply.ok, true, 'e a câmera de antes da queda segue de pé');
    volta.close();
});

test('o token de outra conta não retoma a sessão, mesmo com a chave', async () => {
    // Senão a conta sem `stream` voltaria com o `can` que o Laravel assinou para outra.
    const { volta, retomada } = await retomar(retomando, {
        token: token({ room: salaRetomada, sub: '82', name: 'Outra', can: TUDO }),
        resumeKey: sessao.resumeKey,
    });

    assert.equal(retomada.resumed, false, 'token de outra conta é entrada nova');
    assert.notEqual(retomada.peerId, sessao.peerId, 'e não herda a sessão de ninguém');
    assert.equal(retomada.userId, '82', 'a identidade é a do token');
    volta.close();
    plateia.close();
});

test('a mesma conta entrando de novo derruba a sessão antiga, nesta sala ou em outra', async () => {
    // O app que não conseguiu fechar a conexão velha deixava a pessoa duas vezes na lista.
    const antiga = await abrir();
    await entrar(antiga, { token: token({ room, sub: '60', name: 'Duplicada', can: TUDO }) });

    const nova = await abrir();
    await entrar(nova, { token: token({ room, sub: '60', name: 'Duplicada', can: TUDO }) });
    await espera(300);

    assert.ok(antiga.events.some(evento => evento.event === 'replaced'), 'a sessão antiga fica sabendo que foi substituída');
    assert.equal(antiga.closeCode, 4002, 'e perde o socket');

    let http = await fetch(`${URL_HTTP}/presence`, { headers: signed('GET', '/presence', '') });
    let presenca = (await http.json()).rooms[room].filter(pessoa => pessoa.sub === '60');
    assert.equal(presenca.length, 1, 'a conta aparece uma vez só');

    const outraSala = await abrir();
    await entrar(outraSala, { token: token({ room: 'checkroom002', sub: '60', name: 'Duplicada', can: TUDO }) });
    await espera(300);

    assert.equal(nova.closeCode, 4002, 'entrar em outra sala também derruba a sessão anterior');
    http = await fetch(`${URL_HTTP}/presence`, { headers: signed('GET', '/presence', '') });
    presenca = Object.values((await http.json()).rooms).flat().filter(pessoa => pessoa.sub === '60');
    assert.equal(presenca.length, 1, 'uma conta, uma sessão no servidor inteiro');
    outraSala.close();

    const visitanteA = await abrir();
    const visitanteB = await abrir();
    await entrar(visitanteA, { room, name: 'Visitante', installId: 'inst-duplo' });
    await entrar(visitanteB, { room, name: 'Visitante', installId: 'inst-duplo' });
    await espera(300);

    assert.equal(visitanteA.closeCode, null, 'visitante não derruba ninguém: o installId é escolhido pelo próprio app');
    visitanteA.close();
    visitanteB.close();
});

test('producePlain exige chave e suíte SRTP válidas antes de aceitar o ingest', async () => {
    // Ingest de RTP puro: o app declara o que vai mandar antes de mandar.
    nativo = await abrir();
    await entrar(nativo, { token: token({ room, sub: '13', name: 'Nativo', can: TUDO }) });

    const semChave = await nativo.call('producePlain', {
        kind: 'video',
        source: 'screen',
        rtpParameters: { codecs: [], encodings: [] },
        srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80' },
    });

    assert.equal(semChave.status, 422, 'produzir sem chave SRTP tem de ser recusado');

    const suiteInvalida = await nativo.call('producePlain', {
        kind: 'video',
        source: 'screen',
        rtpParameters: { codecs: [], encodings: [] },
        srtpParameters: { cryptoSuite: 'ROT13', keyBase64: 'AAAA' },
    });

    assert.equal(suiteInvalida.ok, false, 'suíte de criptografia desconhecida tem de ser recusada');

    plain = await nativo.call('producePlain', videoPuro('screen', 0x22345678));

    assert.equal(plain.ok, true, `o ingest puro tem de ser aceito: ${JSON.stringify(plain)}`);
    assert.ok(plain.data.producerId, 'devolve o id do producer');
    assert.ok(plain.data.port > 0, 'devolve a porta UDP para onde mandar o RTP');
    assert.ok(plain.data.srtpParameters?.keyBase64, 'devolve a chave do outro sentido');
});

test('tela e câmera do mesmo peer são aceitas, na mesma porta', async () => {
    // Tela e câmera da MESMA pessoa: dois vídeos no mesmo transport, distinguidos só
    // pelo SSRC. (O `produce` por WebRTC que dá certo fica de fora: precisa de um
    // sendTransport de verdade, que só o mediasoup-client cria.)
    const camera = await nativo.call('producePlain', videoPuro('camera', 0x2234567a));
    assert.equal(camera.ok, true, `a câmera junto da tela tem de ser aceita: ${JSON.stringify(camera)}`);
    assert.notEqual(camera.data.producerId, plain.data.producerId, 'tela e câmera são produtores diferentes');
    assert.equal(camera.data.port, plain.data.port, 'na mesma porta');
});

test('outra pessoa na sala consome a transmissão pura como qualquer outra', async () => {
    // O ponto inteiro: outra pessoa na sala consome como qualquer transmissão.
    assistindo = await abrir();
    const entradaAssiste = await entrar(assistindo, { token: token({ room, sub: '14', name: 'Assiste', can: TUDO }) });

    const videosNativo = entradaAssiste.peers.find(p => p.name === 'Nativo').producers.filter(p => p.kind === 'video');
    assert.deepEqual(videosNativo.map(p => p.source).sort(), ['camera', 'screen'], 'quem entra vê a tela E a câmera de quem já estava');
    assert.equal(new Set(videosNativo.map(p => p.producerId)).size, 2, 'com ids distintos');

    const transporte = await assistindo.call('createTransport');
    consumo = await assistindo.call('consume', {
        transportId: transporte.data.transportId,
        producerId: plain.data.producerId,
        rtpCapabilities: CAPACIDADES,
    });

    assert.equal(consumo.ok, true, `um producer puro precisa ser consumível: ${JSON.stringify(consumo)}`);
});

test('quem transmite descobre quem está assistindo, e sai da lista quem pausou', async () => {
    // Plateia é quem está OLHANDO. O consumer nasce pausado, então nascer não basta:
    // quem conta é o `resume`, e pausar o cartão tira a pessoa da lista.
    await espera(200);

    assert.equal(
        nativo.events.filter(evento => evento.event === 'watchers').length,
        0,
        'consumer pausado não é plateia: ninguém está vendo nada ainda',
    );

    await assistindo.call('resumeConsumer', { consumerId: consumo.data.consumerId });
    await espera(200);

    const plateia = nativo.events.filter(evento => evento.event === 'watchers').at(-1);

    assert.ok(plateia, 'retomar o consumer avisa a sala de quem está assistindo');
    assert.equal(plateia.data.producerId, plain.data.producerId, 'o aviso diz de qual transmissão é a plateia');
    assert.deepEqual(plateia.data.watchers.map(pessoa => pessoa.name), ['Assiste'], 'e quem está assistindo');

    await assistindo.call('pauseConsumer', { consumerId: consumo.data.consumerId });
    await espera(200);

    const vazia = nativo.events.filter(evento => evento.event === 'watchers').at(-1);

    assert.deepEqual(vazia.data.watchers, [], 'pausar o cartão tira a pessoa da plateia');

    // Câmera não tem plateia: seria todo mundo consumindo todo mundo, e o evento viraria
    // enxurrada numa sala cheia.
    const antes = nativo.events.filter(evento => evento.event === 'watchers').length;
    const camera = await nativo.call('producePlain', videoPuro('camera', 0x2234567b));

    assert.equal(camera.ok, true, `a câmera precisa ser aceita: ${JSON.stringify(camera)}`);

    const transporteCamera = await assistindo.call('createTransport');
    const consumoCamera = await assistindo.call('consume', {
        transportId: transporteCamera.data.transportId,
        producerId: camera.data.producerId,
        rtpCapabilities: CAPACIDADES,
    });

    assert.equal(consumoCamera.ok, true, `a câmera precisa ser consumível: ${JSON.stringify(consumoCamera)}`);
    await assistindo.call('resumeConsumer', { consumerId: consumoCamera.data.consumerId });
    await espera(200);

    assert.equal(
        nativo.events.filter(evento => evento.event === 'watchers').length,
        antes,
        'assistir câmera não anuncia plateia nenhuma',
    );

    await assistindo.call('resumeConsumer', { consumerId: consumo.data.consumerId });
});

test('o áudio da mesma transmissão divide a porta com o vídeo', async () => {
    // Áudio da mesma transmissão: mesmo transport, mesma porta. Um transport por mídia
    // gastava o dobro de portas UDP, e cada porta a mais é uma regra de firewall a mais.
    plainAudio = await nativo.call('producePlain', {
        kind: 'audio',
        source: 'screenAudio',
        srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: Buffer.alloc(30, 7).toString('base64') },
        rtpParameters: {
            codecs: [{ mimeType: 'audio/opus', payloadType: 111, clockRate: 48000, channels: 2, rtcpFeedback: [] }],
            encodings: [{ ssrc: 0x22345679 }],
        },
    });

    assert.equal(plainAudio.ok, true, `o áudio puro tem de ser aceito: ${JSON.stringify(plainAudio)}`);
    assert.equal(plainAudio.data.port, plain.data.port, 'áudio e vídeo da mesma transmissão dividem a porta');
    assert.notEqual(plainAudio.data.producerId, plain.data.producerId, 'mas são produtores diferentes');
});

test('consumePlain entrega porta, payload, chave e um ssrc por mídia', async () => {
    // Assistir por RTP puro: o Linux, sem WebRTC na janela, recebe numa porta UDP.
    const semChaveVer = await assistindo.call('consumePlain', { producerId: plain.data.producerId, srtpParameters: {} });
    assert.equal(semChaveVer.status, 422, 'assistir puro sem chave SRTP tem de ser recusado');

    chaveVer = { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: Buffer.alloc(30, 9).toString('base64') };
    verVideo = await assistindo.call('consumePlain', { producerId: plain.data.producerId, srtpParameters: chaveVer });
    assert.equal(verVideo.ok, true, `assistir puro tem de ser aceito: ${JSON.stringify(verVideo)}`);
    assert.ok(verVideo.data.port > 0 && verVideo.data.port !== plain.data.port, 'recebe numa porta própria, não na do ingest');
    assert.ok(verVideo.data.payloadType > 0 && verVideo.data.srtpParameters?.keyBase64, 'diz o payload e a chave para abrir');
    assert.equal(verVideo.data.name, 'Nativo', 'diz de quem é a tela');

    const verAudio = await assistindo.call('consumePlain', { producerId: plainAudio.data.producerId, srtpParameters: chaveVer });
    assert.equal(verAudio.data.port, verVideo.data.port, 'áudio e vídeo chegam pela mesma porta');
    assert.ok(Number.isInteger(verVideo.data.ssrc) && verVideo.data.ssrc > 0, 'e cada um diz o próprio SSRC, para o receptor separar');
    assert.notEqual(verAudio.data.ssrc, verVideo.data.ssrc, 'que não pode ser o mesmo');
});

test('fechar o último producer não derruba o transporte de recepção', async () => {
    // Quem fala pelo Linux envia E recebe por RTP puro. Fechar o último producer solta
    // a porta de envio, mas a de recepção fica: os consumers moram nela.
    const micAssiste = await assistindo.call('producePlain', audioPuro('mic', 0x300));
    assert.equal(micAssiste.ok, true, `quem assiste também produz: ${JSON.stringify(micAssiste)}`);
    const reply = await assistindo.call('closeProducer', { producerId: micAssiste.data.producerId });
    assert.deepEqual(reply.data, { status: 'closed' }, 'e fecha o próprio producer');

    const aindaVer = await assistindo.call('consumePlain', { producerId: plainAudio.data.producerId, srtpParameters: chaveVer });
    assert.equal(aindaVer.ok, true, `fechar o último producer não pode derrubar o transport de recepção: ${JSON.stringify(aindaVer)}`);
    assert.equal(aindaVer.data.port, verVideo.data.port, 'que continua o mesmo');

    const retomado = await assistindo.call('resumeConsumer', { consumerId: verVideo.data.consumerId });
    assert.equal(retomado.ok, true, 'o consumer puro retoma como qualquer outro');
});

test('pausar e retomar o próprio producer avisa a sala, e ninguém pausa o dos outros', async () => {
    // Mutar a si mesmo: pausa o producer e a sala fica sabendo, sem derrubar nada.
    assistindo.events.length = 0;
    let reply = await nativo.call('pauseProducer', { producerId: plain.data.producerId });
    assert.deepEqual(reply.data, { status: 'paused' }, `pausar o próprio producer passa: ${JSON.stringify(reply)}`);

    reply = await nativo.call('resumeProducer', { producerId: plain.data.producerId });
    assert.deepEqual(reply.data, { status: 'resumed' }, 'e retomar também');

    reply = await assistindo.call('pauseProducer', { producerId: plain.data.producerId });
    assert.equal(reply.status, 404, 'ninguém pausa o producer dos outros');

    await espera(300);
    assert.ok(
        assistindo.events.some(e => e.event === 'producerPaused' && e.data.producerId === plain.data.producerId && e.data.peerId),
        'quem assiste fica sabendo da pausa',
    );
    assert.ok(assistindo.events.some(e => e.event === 'producerResumed'), 'e da retomada');
});

test('closeConsumer fecha o consumer, e da segunda vez ele já não existe', async () => {
    let reply = await assistindo.call('closeConsumer', { consumerId: consumo.data.consumerId });
    assert.deepEqual(reply.data, { status: 'closed' }, 'fechar o consumer passa');

    reply = await assistindo.call('closeConsumer', { consumerId: consumo.data.consumerId });
    assert.equal(reply.status, 404, 'e ele some de vez');
});

test('o mute assinado pausa só o mic daquela conta, e o silêncio gruda', async () => {
    // Silenciar vem do Laravel, por HTTP assinado, e só mexe no mic daquela conta.
    const mic = await nativo.call('producePlain', audioPuro('mic', 0x22345680));
    assert.equal(mic.ok, true, `o mic puro tem de ser aceito: ${JSON.stringify(mic)}`);

    const mutePath = `/rooms/${room}/mute`;
    const muteBody = JSON.stringify({ userId: '13', muted: true });

    let http = await fetch(`${URL_HTTP}${mutePath}`, { method: 'POST', body: muteBody, headers: { 'content-type': 'application/json' } });
    assert.equal(http.status, 401, 'silenciar sem assinatura tem de ser recusado');

    assistindo.events.length = 0;
    nativo.events.length = 0;
    http = await fetch(`${URL_HTTP}${mutePath}`, { method: 'POST', body: muteBody, headers: signed('POST', mutePath, muteBody) });
    assert.deepEqual(await http.json(), { muted: 1 }, 'silenciar assinado pausa só o mic');

    // E o silêncio gruda: a pessoa não retoma nem produz outro mic enquanto durar.
    let reply = await nativo.call('resumeProducer', { producerId: mic.data.producerId });
    assert.equal(reply.status, 403, 'silenciado pelo servidor não retoma o próprio mic');
    reply = await nativo.call('producePlain', audioPuro('mic', 0x22345681));
    assert.equal(reply.status, 403, 'nem abre outro mic');
    reply = await nativo.call('resumeProducer', { producerId: plain.data.producerId });
    assert.equal(reply.ok, true, 'mas a tela continua livre');
    assert.ok(nativo.events.some(e => e.event === 'serverMuted' && e.data.muted === true), 'e quem foi silenciado fica sabendo');

    const unmuteBody = JSON.stringify({ userId: '13', muted: false });
    http = await fetch(`${URL_HTTP}${mutePath}`, { method: 'POST', body: unmuteBody, headers: signed('POST', mutePath, unmuteBody) });
    assert.deepEqual(await http.json(), { muted: 1 }, 'e devolve a voz');

    reply = await nativo.call('resumeProducer', { producerId: mic.data.producerId });
    assert.equal(reply.ok, true, 'devolvida a voz, o mic volta a obedecer');

    await espera(300);
    assert.ok(
        assistindo.events.some(e => e.event === 'producerPaused' && e.data.producerId === mic.data.producerId),
        'a sala sabe que o mic foi silenciado',
    );
    assert.ok(assistindo.events.some(e => e.event === 'producerResumed' && e.data.producerId === mic.data.producerId), 'e que voltou');
});

test('o /presence assinado lista quem está na sala e o que cada um produz', async () => {
    // Quem está em cada sala, para o site desenhar a lista de voz.
    let http = await fetch(`${URL_HTTP}/presence`);
    assert.equal(http.status, 401, 'presença sem assinatura tem de ser recusada');

    http = await fetch(`${URL_HTTP}/presence`, { headers: signed('GET', '/presence', '') });
    assert.equal(http.status, 200, 'presença assinada passa');

    const presenca = (await http.json()).rooms[room].find(p => p.sub === '13');
    assert.equal(presenca?.name, 'Nativo', 'a sala lista quem está nela');
    assert.deepEqual([...presenca.sources].sort(), ['camera', 'mic', 'screen', 'screenAudio'], 'com o que cada um está produzindo');
});

test('sair no botão avisa a sala na hora, sem esperar a carência', async () => {
    // Sair de propósito não deixa fantasma: a sala avisa na hora.
    assistindo.events.length = 0;
    await nativo.call('leave');
    await espera(300);

    assert.ok(
        assistindo.events.some(evento => evento.event === 'peerLeft'),
        'sair no botão avisa a sala na hora, sem esperar a carência',
    );
});

test('o kick assinado derruba a sessão da conta, e a assinatura errada não derruba ninguém', async () => {
    // Expulsar vem do Laravel, por HTTP assinado. Sem assinatura, ou com a hora fora da
    // janela, a porta 3000 não expulsa ninguém.
    const kickPath = `/rooms/${room}/kick`;
    const kickBody = JSON.stringify({ userId: '14' });

    let http = await fetch(`${URL_HTTP}${kickPath}`, { method: 'POST', body: kickBody, headers: { 'content-type': 'application/json' } });
    assert.equal(http.status, 401, 'expulsar sem assinatura tem de ser recusado');

    http = await fetch(`${URL_HTTP}${kickPath}`, { method: 'POST', body: kickBody, headers: signed('POST', kickPath, kickBody, '1000') });
    assert.equal(http.status, 401, 'assinatura com hora velha tem de ser recusada');

    http = await fetch(`${URL_HTTP}${kickPath}`, { method: 'POST', body: kickBody, headers: signed('POST', kickPath, '{"userId":"10"}') });
    assert.equal(http.status, 401, 'assinatura de outro corpo tem de ser recusada');

    http = await fetch(`${URL_HTTP}${kickPath}`, { method: 'POST', body: 'nao-json', headers: signed('POST', kickPath, 'nao-json') });
    assert.equal(http.status, 422, 'corpo que não é JSON é erro de validação, não 500');

    dono.events.length = 0;
    http = await fetch(`${URL_HTTP}${kickPath}`, { method: 'POST', body: kickBody, headers: signed('POST', kickPath, kickBody) });
    assert.equal(http.status, 200, 'expulsar assinado passa');
    assert.deepEqual(await http.json(), { kicked: 1 }, 'e derruba a sessão daquela conta');

    await espera(300);
    assert.ok(assistindo.events.some(evento => evento.event === 'kicked'), 'quem foi expulso fica sabendo');
    assert.equal(assistindo.closeCode, 4001, 'e perde o socket: a sessão expulsa não pode seguir alocando');
    assert.ok(dono.events.some(evento => evento.event === 'peerKicked'), 'e a sala também');

    http = await fetch(`${URL_HTTP}/rooms/sala-que-nao-existe/kick`, { method: 'POST', body: kickBody, headers: signed('POST', '/rooms/sala-que-nao-existe/kick', kickBody) });
    assert.deepEqual(await http.json(), { kicked: 0 }, 'sala fora do ar não tem quem expulsar, e não é erro');
});

test('remover alguém ao vivo pelo socket é recusado', async () => {
    // Quem já caiu pode ser tirado da lista por qualquer um; quem está ao vivo, não.
    const reply = await dono.call('removePeer', { peerId: entrada.peerId });
    assert.equal(reply.status, 422, 'remover alguém ao vivo pelo socket é recusado — isso é do Laravel');
});

/**
 * Um SFU só para um cenário, com a configuração que o servidor de todos não pode ter (o
 * Laravel de mentira do webhook, o heartbeat de 200 ms). Sempre com faixa de portas
 * própria, para não disputar com o servidor que já está no ar. Quem chama mata com
 * SIGKILL: o mediasoup engole o primeiro SIGTERM.
 */
const startSfu = async (port, env) => {
    const server = spawn('node', ['dist/server.js'], {
        env: { ...process.env, SFU_PORT: String(port), SFU_SECRET: SECRET, SFU_WORKERS: '1', ...env },
        stdio: ['ignore', 'pipe', 'inherit'],
    });

    await new Promise((resolve, reject) => {
        server.stdout.on('data', chunk => String(chunk).includes('SFU em') && resolve());
        server.on('exit', code => reject(new Error(`o SFU da porta ${port} saiu antes de subir (${code})`)));
        setTimeout(() => reject(new Error(`o SFU da porta ${port} não subiu em 15s`)), 15_000).unref();
    });

    return server;
};

/** O aviso ao Laravel de quem entrou e saiu. */
test('o webhook avisa o Laravel de quem entrou e saiu, assinado', async () => {
    const recebidos = [];
    const laravel = createServer((request, response) => {
        let body = '';
        request.on('data', chunk => (body += chunk));
        request.on('end', () => {
            recebidos.push({ path: request.url, headers: request.headers, body });
            response.end('{}');
        });
    });

    await new Promise(resolve => laravel.listen(0, '127.0.0.1', resolve));

    const port = 3197;
    const server = await startSfu(port, {
        SFU_MEDIA_PORT: '40500',
        SFU_PLAIN_PORT: '42000',
        SFU_LARAVEL_URL: `http://127.0.0.1:${laravel.address().port}`,
    });

    try {
        const cliente = new Client(`ws://127.0.0.1:${port}/sfu`);
        await cliente.open();

        const anonimo = new Client(`ws://127.0.0.1:${port}/sfu`);
        await anonimo.open();
        await entrar(anonimo, { room: 'webhookroom', name: 'Anônimo', installId: 'inst-webhook' });
        await espera(300);

        await entrar(cliente, { token: token({ room: 'webhookroom', sub: 'user:12', name: 'Edsu', can: TUDO }) });
        await cliente.call('leave');
        await espera(500);

        assert.equal(recebidos.length, 3, `visitante avisa a entrada para a auditoria; conta avisa entrada e saída: ${JSON.stringify(recebidos)}`);

        // O aviso é fire-and-forget: o `joined` e o `left` saem em requisições soltas, e
        // qual delas chega primeiro ao Laravel não é garantido. Procurar pelo evento, e não
        // pela ordem, é o que separa este cenário de um teste que passa por sorte.
        const avisos = recebidos.map(item => ({ ...item, corpo: JSON.parse(item.body) }));
        const visitante = avisos.find(aviso => aviso.corpo.sub === 'guest:inst-webhook');

        assert.equal(visitante?.corpo.event, 'joined', 'o visitante avisa a entrada');
        assert.equal(visitante.corpo.name, 'Anônimo', 'com o nome que digitou');

        for (const evento of ['joined', 'left']) {
            const aviso = avisos.find(item => item.corpo.sub === 'user:12' && item.corpo.event === evento);

            assert.ok(aviso, `falta o aviso de ${evento} da conta: ${JSON.stringify(recebidos)}`);
            assert.equal(aviso.path, '/api/sfu/events', 'bate na rota do Laravel');
            assert.equal(aviso.corpo.name, 'Edsu', 'com o nome, que é o que a auditoria mostra');
            assert.equal(aviso.corpo.room, 'webhookroom', 'e de qual sala');
            assert.equal(aviso.corpo.ip, '127.0.0.1', 'e de onde a pessoa veio');
            assert.equal(
                aviso.headers['x-unkvoid-signature'],
                hmac(`${aviso.headers['x-unkvoid-timestamp']}\nPOST\n/api/sfu/events\n${aviso.body}`),
                'assinado como o token, para o Laravel conferir',
            );
        }

        cliente.close();
        anonimo.close();
    } finally {
        // SIGKILL, não SIGTERM: o mediasoup registra `process.once('SIGTERM')` dentro do
        // SFU, então o primeiro TERM morre no handler dele e o processo fica de pé
        // segurando o runner de teste para sempre.
        server.kill('SIGKILL');
        laravel.close();
    }
});

/**
 * Um cliente que não responde ao ping é indistinguível de um cliente vivo, do ponto de
 * vista do TCP, quando a queda é suja: a tampa do notebook fecha e nunca chega FIN nem
 * RST. Enquanto o servidor não perguntava, essa pessoa ficava ativa na sala para sempre,
 * o router do mediasoup nunca era devolvido e as portas de RTP puro dela também não.
 * Este cenário existe porque esse vazamento não aparece em nenhum teste de caminho feliz.
 */
test('o heartbeat derruba o socket mudo, e o socket vivo fica', async () => {
    const port = 3199;
    const heartbeatMs = 200;
    const server = await startSfu(port, {
        SFU_MEDIA_PORT: '40600',
        SFU_PLAIN_PORT: '42100',
        SFU_HEARTBEAT_MS: String(heartbeatMs),
    });
    const sockets = [];

    const conectar = options => new Promise((resolve, reject) => {
        const socket = new WsSocket(`ws://127.0.0.1:${port}/sfu`, options);

        sockets.push(socket);
        socket.on('open', () => resolve(socket));
        socket.on('error', reject);
    });

    /** Se o socket fechou dentro do prazo. O prazo não segura o processo depois do cenário. */
    const fechouEm = (socket, ms) => new Promise(resolve => {
        socket.on('close', () => resolve(true));
        setTimeout(() => resolve(false), ms).unref();
    });

    try {
        // `autoPong: false` é o cliente fingindo estar morto sem fechar a conexão. É
        // exatamente o que um notebook com a tampa fechada parece, visto daqui.
        const mudo = await conectar({ autoPong: false });

        assert.equal(await fechouEm(mudo, heartbeatMs * 15), true, 'quem não responde ao ping precisa perder a conexão');

        // E quem responde continua de pé: derrubar todo mundo seria o oposto do conserto.
        const vivo = await conectar();

        assert.equal(await fechouEm(vivo, heartbeatMs * 10), false, 'quem responde ao ping não pode ser derrubado junto');
    } finally {
        for (const socket of sockets) {
            socket.terminate();
        }

        server.kill('SIGKILL');
    }
});
