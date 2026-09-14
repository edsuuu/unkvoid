/**
 * O contrato do SFU, conferido contra um servidor de verdade.
 *
 * Sobe o SFU (`npm run build && SFU_CONNECTIONS_PER_MINUTE=200 node dist/server.js`) e
 * rode `npm run check`. O teto de conexões precisa ser levantado porque a conferência
 * abre uma dúzia de clientes de uma vez, que é exatamente o que ele existe para barrar.
 */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { createSocket } from 'node:dgram';
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { Readable } from 'node:stream';
import { after, before, test } from 'node:test';

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
 * Um SFU só para um cenário, apontado para servidores HTTP daqui — como faz o
 * check-heartbeat. Quem chama mata com SIGKILL: o mediasoup engole o primeiro SIGTERM.
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
        await entrar(anonimo, { room: 'webhookroom', name: 'Anônimo' });

        await entrar(cliente, { token: token({ room: 'webhookroom', sub: 'user:12', name: 'Edsu', can: TUDO }) });
        await cliente.call('leave');
        await espera(500);

        assert.equal(recebidos.length, 2, `sala anônima não avisa; conta avisa entrada e saída: ${JSON.stringify(recebidos)}`);

        for (const [indice, evento] of ['joined', 'left'].entries()) {
            const aviso = recebidos[indice];
            const corpo = JSON.parse(aviso.body);

            assert.equal(aviso.path, '/api/sfu/events', 'bate na rota do Laravel');
            assert.equal(corpo.event, evento, `o ${indice + 1}º aviso é ${evento}`);
            assert.equal(corpo.sub, 'user:12', 'diz de qual conta');
            assert.equal(corpo.room, 'webhookroom', 'e de qual sala');
            assert.equal(corpo.ip, '127.0.0.1', 'e de onde a pessoa veio');
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

const FFMPEG = process.env.SFU_FFMPEG ?? 'ffmpeg';
const FFPROBE = FFMPEG.includes('/') ? join(dirname(FFMPEG), 'ffprobe') : 'ffprobe';

/** Roda até o fim e devolve a saída inteira. */
const execute = (command, args) =>
    new Promise((resolve, reject) => {
        const child = spawn(command, args);
        let stdout = '';
        let stderr = '';

        child.stdout.on('data', chunk => (stdout += chunk));
        child.stderr.on('data', chunk => (stderr += chunk));
        child.on('error', reject);
        child.on('exit', code => resolve({ code, stdout, stderr }));
    });

const until = async (probe, ms) => {
    for (const deadline = Date.now() + ms; Date.now() < deadline; await espera(200)) {
        const found = probe();

        if (found) {
            return found;
        }
    }

    throw new Error(`nada em ${ms} ms`);
};

/**
 * Onde cada clarão da tela e cada bipe caem no clipe. A fonte pisca a cada 5 s com o bipe
 * do áudio da tela junto, e o mic bipa 2,5 s depois de cada clarão: a distância medida é
 * o erro de sincronia.
 */
const markers = async file => {
    const { stderr } = await execute(FFMPEG, [
        '-nostdin', '-i', file,
        '-filter_complex', '[0:v]signalstats,metadata=mode=print:key=lavfi.signalstats.YAVG[v];[0:a]silencedetect=n=-30dB:d=0.02[a]',
        '-map', '[v]', '-map', '[a]', '-f', 'null', '-',
    ]);
    const flashes = [];
    const beeps = [];
    let at = 0;
    let previous = 0;

    for (const line of stderr.split('\n')) {
        const frame = line.match(/pts_time:([\d.]+)/);
        const brightness = line.match(/YAVG=([\d.]+)/);
        const beep = line.match(/silence_end: ([\d.]+)/);

        if (frame) {
            at = Number(frame[1]);
        } else if (brightness) {
            if (Number(brightness[1]) > 100 && previous <= 100) {
                flashes.push(at);
            }

            previous = Number(brightness[1]);
        } else if (beep) {
            beeps.push(Number(beep[1]));
        }
    }

    const offsets = phase => flashes
        .map(flash => beeps.map(beep => beep - flash - phase).find(delta => Math.abs(delta) < 0.3))
        .filter(delta => delta !== undefined);

    return { flashes, screenAudio: offsets(0), mic: offsets(2.5) };
};

test('o clipe grava a tela de quem transmite num canal, mistura o áudio e sobe para o bucket', async () => {
    const recordings = mkdtempSync(join(tmpdir(), 'unkvoid-recordings-'));
    const bucket = mkdtempSync(join(tmpdir(), 'unkvoid-bucket-'));
    const clipId = '01k9c1ip000000000000000000';
    const prefix = `clips/${clipId}/`;
    const fields = { policy: 'cG9saWN5', 'x-amz-signature': 'assinatura' };
    const events = [];

    const http = createServer(async (request, response) => {
        const body = new Response(Readable.toWeb(request), { headers: { 'content-type': request.headers['content-type'] ?? '' } });

        if (request.url === '/api/sfu/events') {
            events.push({ headers: request.headers, body: await body.text() });
            response.end('{}');

            return;
        }

        // Como o MinIO: os campos da política, a key, o arquivo por último, e nada a mais.
        const form = await body.formData();
        const names = [...form.keys()];
        const key = String(form.get('key'));
        const accepted = names.every(name => name in fields || name === 'key' || name === 'file')
            && Object.keys(fields).every(name => names.includes(name))
            && names.at(-1) === 'file'
            && names.indexOf('key') < names.indexOf('file')
            && key.startsWith(prefix);

        if (!accepted) {
            response.statusCode = 403;
            response.end(`campos recusados: ${names.join(',')}`);

            return;
        }

        writeFileSync(join(bucket, key.slice(prefix.length)), Buffer.from(await form.get('file').arrayBuffer()));
        response.statusCode = 204;
        response.end();
    });

    await new Promise(resolve => http.listen(0, '127.0.0.1', resolve));

    const relay = createSocket('udp4');

    await new Promise(resolve => relay.bind(0, '127.0.0.1', resolve));

    const port = 3196;
    const laravel = `http://127.0.0.1:${http.address().port}`;
    const channel = '01k9c1ipr00m00000000000000';
    const rings = join(recordings, `unkvoid-sfu-${port}`, 'rings');
    const server = await startSfu(port, {
        SFU_MEDIA_PORT: '40600',
        SFU_PLAIN_PORT: '42100',
        SFU_LARAVEL_URL: laravel,
        SFU_RECORDINGS_DIR: recordings,
    });
    const withoutFfmpeg = await startSfu(3195, {
        SFU_MEDIA_PORT: '40700',
        SFU_PLAIN_PORT: '42200',
        SFU_LARAVEL_URL: '',
        SFU_FFMPEG: '/nao/existe/ffmpeg',
    });
    let sender;

    const askClip = (order, room = channel, target = port) => {
        const path = `/rooms/${room}/clips`;
        const body = JSON.stringify({ clipId, upload: { url: `${laravel}/bucket`, fields, prefix }, ...order });

        return fetch(`http://127.0.0.1:${target}${path}`, { method: 'POST', body, headers: signed('POST', path, body) });
    };

    try {
        let http503 = await askClip({ clipper: 'user:41', streamer: 'user:40' }, channel, 3195);
        assert.equal(http503.status, 503, 'sem ffmpeg executável o pedido de clipe é 503');
        withoutFfmpeg.kill('SIGKILL');

        const connect = async () => {
            const client = new Client(`ws://127.0.0.1:${port}/sfu`);

            await client.open();
            abertos.push(client);

            return client;
        };

        const streamer = await connect();
        const clipper = await connect();
        const guest = await connect();

        await entrar(streamer, { token: token({ room: channel, sub: 'user:40', name: 'Transmite', can: TUDO }) });
        await entrar(clipper, { token: token({ room: channel, sub: 'user:41', name: 'Clipa', can: TUDO }) });
        await entrar(guest, { room: 'clipsanon01', name: 'Visitante', installId: 'inst-clip' });

        const screen = await streamer.call('producePlain', videoPuro('screen', 0x5000));
        assert.equal(screen.ok, true, `a tela tem de ser aceita: ${JSON.stringify(screen)}`);
        assert.equal((await streamer.call('producePlain', audioPuro('screenAudio', 0x5001))).ok, true, 'e o áudio da tela');
        assert.equal((await guest.call('producePlain', videoPuro('screen', 0x6000))).ok, true, 'a sala anônima também transmite');

        // O ingest aprende um endereço só: as três mídias saem por um socket, como no app.
        relay.on('message', (packet, from) => from.port !== screen.data.port && relay.send(packet, screen.data.port, '127.0.0.1'));

        const srtp = ['-srtp_out_suite', 'AES_CM_128_HMAC_SHA1_80', '-srtp_out_params', Buffer.alloc(30, 7).toString('base64')];
        const destination = `srtp://127.0.0.1:${relay.address().port}`;
        const opus = ssrc => ['-ac', '2', '-c:a', 'libopus', '-b:a', '64k', '-f', 'rtp', '-payload_type', '111', '-ssrc', String(ssrc), ...srtp, destination];
        const startedAt = Date.now();
        const waitUntil = second => espera(Math.max(0, startedAt + second * 1000 - Date.now()));

        sender = spawn(FFMPEG, [
            '-nostdin', '-loglevel', 'error',
            '-re', '-t', '60', '-f', 'lavfi', '-i', "color=c=black:s=640x360:r=30,drawbox=c=white:t=fill:enable='lt(mod(t\\,5)\\,0.1)'",
            '-re', '-t', '60', '-f', 'lavfi', '-i', "aevalsrc='if(lt(mod(t,5),0.1),0.5*sin(2*PI*440*t),0)':s=48000",
            '-re', '-t', '60', '-f', 'lavfi', '-i', "aevalsrc='if(between(mod(t,5),2.5,2.6),0.5*sin(2*PI*880*t),0)':s=48000",
            '-map', '0:v', '-c:v', 'libx264', '-preset', 'ultrafast', '-tune', 'zerolatency', '-bf', '0', '-g', '30', '-profile:v', 'baseline',
            '-f', 'rtp', '-payload_type', '96', '-ssrc', String(0x5000), ...srtp, destination,
            '-map', '1:a', ...opus(0x5001),
            '-map', '2:a', ...opus(0x5002),
        ], { stdio: ['ignore', 'ignore', 'inherit'] });

        // O mic aparece depois da tela e fica mudo no meio: o anel da tela não pode esperar.
        await waitUntil(4);
        const mic = await streamer.call('producePlain', audioPuro('mic', 0x5002));
        assert.equal(mic.ok, true, `o mic tem de ser aceito: ${JSON.stringify(mic)}`);
        await waitUntil(10);
        await streamer.call('pauseProducer', { producerId: mic.data.producerId });
        await waitUntil(16);
        await streamer.call('resumeProducer', { producerId: mic.data.producerId });
        await waitUntil(26);

        assert.equal(readdirSync(rings).length, 1, 'só a conta logada num canal grava: a sala anônima não abre anel');

        let reply = await askClip({ clipper: 'user:99', streamer: 'user:40' });
        assert.equal(reply.status, 403, 'quem pede o clipe precisa estar na sala');

        reply = await askClip({ clipper: 'user:41', streamer: 'user:41' });
        assert.equal(reply.status, 404, 'quem não transmite não tem anel');

        reply = await askClip({ clipper: 'guest:inst-clip', streamer: 'guest:inst-clip' }, 'clipsanon01');
        assert.equal(reply.status, 404, 'sala anônima nunca tem anel');

        reply = await askClip({ clipId: '../fora', clipper: 'user:41', streamer: 'user:40' });
        assert.equal(reply.status, 422, 'o id do clipe vira pasta: só letra e número');

        const clipAt = Date.now();
        reply = await askClip({ clipper: 'user:41', streamer: 'user:40' });
        assert.equal(reply.status, 202, 'quem está na sala clipa quem transmite');
        assert.deepEqual(await reply.json(), { accepted: true });

        reply = await askClip({ clipper: 'user:41', streamer: 'user:40' });
        assert.equal(reply.status, 202, 'o Laravel repete o pedido e ouve 202 de novo');

        const clipEvents = () => events.map(event => ({ ...event, data: JSON.parse(event.body) })).filter(event => event.data.event.startsWith('clip.'));
        const ready = (await until(() => clipEvents()[0], 120_000));
        const readyInMs = Date.now() - clipAt;
        const streamedSeconds = (clipAt - startedAt) / 1000;

        assert.equal(ready.data.event, 'clip.ready', `o clipe tem de ficar pronto: ${ready.body}`);
        assert.equal(ready.data.clipId, clipId);
        assert.equal(
            ready.headers['x-unkvoid-signature'],
            hmac(`${ready.headers['x-unkvoid-timestamp']}\nPOST\n/api/sfu/events\n${ready.body}`),
            'o clip.ready vai assinado como o joined',
        );

        const uploaded = readdirSync(bucket);

        for (const name of ['index.m3u8', 'seg-000.ts', 'thumb.jpg', 'clip.mp4']) {
            assert.ok(uploaded.includes(name), `${name} chega no bucket sob o prefixo: ${uploaded.join(', ')}`);
        }

        assert.equal(
            ready.data.sizeBytes,
            uploaded.reduce((total, name) => total + statSync(join(bucket, name)).size, 0),
            'sizeBytes é a soma de tudo o que subiu',
        );
        assert.match(readFileSync(join(bucket, 'index.m3u8'), 'utf8'), /#EXT-X-PLAYLIST-TYPE:VOD/);

        const probe = async file => JSON.parse((await execute(FFPROBE, [
            '-v', 'error', '-show_entries', 'stream=codec_name:format=duration', '-of', 'json', file,
        ])).stdout);
        const playlist = await probe(join(bucket, 'index.m3u8'));
        const download = await probe(join(bucket, 'clip.mp4'));
        const seconds = Number(download.format.duration);

        assert.deepEqual(playlist.streams.map(stream => stream.codec_name).sort(), ['aac', 'h264'], 'o HLS tem H.264 e AAC');
        assert.deepEqual(download.streams.map(stream => stream.codec_name).sort(), ['aac', 'h264'], 'o MP4 também');
        assert.ok(Math.abs(ready.data.durationMs / 1000 - seconds) < 0.2, `a duração do HLS (${ready.data.durationMs} ms) é a do MP4 (${seconds} s)`);
        assert.ok(seconds <= 300 && seconds <= streamedSeconds && seconds > streamedSeconds - 8, `${seconds} s de clipe para ${streamedSeconds} s transmitidos`);

        const { flashes, screenAudio, mic: micOffsets } = await markers(join(bucket, 'clip.mp4'));
        const worst = Math.max(...[...screenAudio, ...micOffsets].map(Math.abs));

        console.log(`# clipe de ${seconds} s pronto em ${readyInMs} ms; ${flashes.length} clarões; atraso áudio da tela ${screenAudio.map(delta => Math.round(delta * 1000)).join('/')} ms; mic ${micOffsets.map(delta => Math.round(delta * 1000)).join('/')} ms`);
        assert.ok(screenAudio.length >= 3, 'o áudio da tela está no clipe, junto dos clarões');
        assert.ok(micOffsets.length >= 2, 'o mic está no clipe, antes e depois de ficar mudo');
        assert.ok(worst <= 0.1, `áudio e vídeo em sincronia (pior caso ${Math.round(worst * 1000)} ms)`);

        await espera(1000);
        assert.equal(clipEvents().length, 1, 'o pedido repetido não gerou um segundo clipe');
        assert.deepEqual(readdirSync(join(recordings, `unkvoid-sfu-${port}`, 'clips')), [], 'o clipe não deixa temporário');

        await streamer.call('closeProducer', { producerId: screen.data.producerId });
        await espera(1500);
        assert.deepEqual(readdirSync(rings), [], 'parar a tela apaga o anel');
    } finally {
        sender?.kill('SIGKILL');
        relay.close();
        server.kill('SIGKILL');
        withoutFfmpeg.kill('SIGKILL');
        http.close();
        rmSync(recordings, { recursive: true, force: true });
        rmSync(bucket, { recursive: true, force: true });
    }
});
