/**
 * O contrato do SFU, conferido contra um servidor de verdade.
 *
 * Sobe o SFU (`npm run build && SFU_CONNECTIONS_PER_MINUTE=200 node dist/server.js`) e
 * rode `npm run check`. O teto de conexões precisa ser levantado porque a conferência
 * abre uma dúzia de clientes de uma vez, que é exatamente o que ele existe para barrar.
 */
import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';

const URL_WS = process.env.SFU_CHECK_URL ?? 'ws://127.0.0.1:3000/sfu';
const URL_HTTP = URL_WS.replace(/^ws/, 'http').replace(/\/sfu$/, '');
const SECRET = process.env.SFU_SECRET ?? 'segredo-de-teste-com-mais-de-32-caracteres';

const hmac = input => createHmac('sha256', SECRET).update(input).digest('hex');

/** O que o Laravel faz: assina quem entra, com que nome, e se é dono. */
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

const espera = ms => new Promise(resolve => setTimeout(resolve, ms));

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

/** Entra na sala e devolve a resposta já conferida. */
const entrar = async (cliente, data) => {
    const reply = await cliente.call('join', data);

    assert.equal(reply.ok, true, `join deveria passar: ${JSON.stringify(reply)}`);

    return reply.data;
};

const abrir = async () => {
    const cliente = new Client();

    await cliente.open();

    return cliente;
};

const run = async () => {
    const room = 'checkroom001';

    const visitante = await abrir();

    let reply = await visitante.call('join', {});
    assert.equal(reply.status, 422, 'entrar sem token é erro de validação');

    reply = await visitante.call('join', { token: 'lixo' });
    assert.equal(reply.status, 422, 'token sem o formato corpo.assinatura é erro de validação');

    reply = await visitante.call('join', { token: token({ room, sub: '1', name: 'X', owner: false }, 'outro-segredo') });
    assert.equal(reply.status, 401, 'token assinado com outro segredo tem de ser recusado');

    reply = await visitante.call('join', { token: token({ room, sub: '1', name: 'X', owner: false, exp: 1 }) });
    assert.equal(reply.status, 401, 'token vencido tem de ser recusado');

    reply = await visitante.call('createTransport', {});
    assert.equal(reply.status, 401, 'ação sem sessão deve dar 401');

    // O app de hoje ainda entra sem token, como visitante. Some quando todos souberem pedir um.
    const antigo = await abrir();
    const legado = await entrar(antigo, { room, name: 'Legado', installId: 'inst-1' });
    assert.equal(legado.owner, false, 'sem token ninguém é dono');
    assert.equal(legado.userId, 'guest:inst-1', 'e a identidade é a instalação');
    antigo.close();

    // A identidade nasce no servidor: ninguém escolhe o próprio id.
    const dono = await abrir();
    const entrada = await entrar(dono, { token: token({ room, sub: '10', name: 'Dono', owner: true }) });

    assert.equal(entrada.owner, true, 'quem o Laravel disse que é dono chega como dono');
    assert.equal(entrada.userId, '10', 'e sabe qual conta é');

    assert.ok(entrada.peerId, 'o servidor devolve o id do participante');
    assert.ok(entrada.resumeKey, 'e a chave para voltar depois de uma queda');
    assert.notEqual(entrada.peerId, entrada.resumeKey, 'a chave não pode ser o id, que a sala inteira conhece');
    assert.ok(entrada.routerRtpCapabilities.codecs.length > 0, 'e as capacidades do router');
    assert.equal(entrada.resumed, false, 'a primeira entrada não é retomada');

    reply = await dono.call('join', { token: token({ room, sub: '10', name: 'Dono', owner: true }) });
    assert.equal(reply.ok, false, 'não dá para entrar duas vezes no mesmo socket');

    reply = await dono.call('pauseConsumer', { consumerId: 'nao-existe' });
    assert.equal(reply.status, 404, 'pausar consumer inexistente deve dar 404');

    reply = await dono.call('acaoQueNaoExiste', {});
    assert.equal(reply.status, 404, 'ação desconhecida deve dar 404');

    reply = await dono.call('createTransport', {});
    assert.equal(reply.ok, true, 'createTransport deve passar');
    assert.ok(reply.data.iceCandidates.some(c => c.protocol === 'udp'), 'precisa anunciar candidato UDP');

    // Ninguém derruba ninguém sabendo o id alheio: sem a chave, é entrada nova.
    const impostor = await abrir();
    const outraSessao = await entrar(impostor, {
        token: token({ room, sub: '11', name: 'Impostor', owner: false }),
        resumeKey: entrada.peerId,
        resume: true,
    });

    assert.equal(outraSessao.resumed, false, 'o peerId de outro não retoma sessão nenhuma');
    assert.notEqual(outraSessao.peerId, entrada.peerId, 'e nem rouba o id');
    impostor.close();

    // Queda de sinalização não tira ninguém da sala: voltar dentro da carência retoma
    // a sessão com a mídia intacta.
    const solucador = await abrir();
    const soluco = await entrar(solucador, { token: token({ room, sub: '12', name: 'Soluço', owner: false }) });

    reply = await solucador.call('createTransport', {});
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
        token: token({ room, sub: '12', name: 'Soluço', owner: false }),
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

    const reaberto = await abrir();
    const limpa = await entrar(reaberto, {
        token: token({ room, sub: '12', name: 'Soluço', owner: false }),
        resumeKey: soluco.resumeKey,
    });

    assert.equal(limpa.resumed, false, 'sem resume:true NÃO pode retomar — o cliente não tem transporte');
    reaberto.close();

    // Ingest de RTP puro: o app declara o que vai mandar antes de mandar.
    const nativo = await abrir();
    await entrar(nativo, { token: token({ room, sub: '13', name: 'Nativo', owner: false }) });

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

    const plain = await nativo.call('producePlain', {
        kind: 'video',
        source: 'screen',
        srtpParameters: { cryptoSuite: 'AES_CM_128_HMAC_SHA1_80', keyBase64: Buffer.alloc(30, 7).toString('base64') },
        rtpParameters: {
            codecs: [{
                mimeType: 'video/H264',
                payloadType: 96,
                clockRate: 90000,
                parameters: { 'packetization-mode': 1, 'level-asymmetry-allowed': 1, 'profile-level-id': '42e01f' },
                rtcpFeedback: [{ type: 'nack' }, { type: 'nack', parameter: 'pli' }],
            }],
            encodings: [{ ssrc: 0x22345678 }],
        },
    });

    assert.equal(plain.ok, true, `o ingest puro tem de ser aceito: ${JSON.stringify(plain)}`);
    assert.ok(plain.data.producerId, 'devolve o id do producer');
    assert.ok(plain.data.port > 0, 'devolve a porta UDP para onde mandar o RTP');
    assert.ok(plain.data.srtpParameters?.keyBase64, 'devolve a chave do outro sentido');

    // O ponto inteiro: outra pessoa na sala consome como qualquer transmissão.
    const assistindo = await abrir();
    await entrar(assistindo, { token: token({ room, sub: '14', name: 'Assiste', owner: false }) });

    const transporte = await assistindo.call('createTransport');
    const consumo = await assistindo.call('consume', {
        transportId: transporte.data.transportId,
        producerId: plain.data.producerId,
        rtpCapabilities: CAPACIDADES,
    });

    assert.equal(consumo.ok, true, `um producer puro precisa ser consumível: ${JSON.stringify(consumo)}`);

    // Áudio da mesma transmissão: mesmo transport, mesma porta. Um transport por mídia
    // gastava o dobro de portas UDP, e cada porta a mais é uma regra de firewall a mais.
    const plainAudio = await nativo.call('producePlain', {
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

    // Sair de propósito não deixa fantasma: a sala avisa na hora.
    assistindo.events.length = 0;
    await nativo.call('leave');
    await espera(300);

    assert.ok(
        assistindo.events.some(evento => evento.event === 'peerLeft'),
        'sair no botão avisa a sala na hora, sem esperar a carência',
    );

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

    dono.events.length = 0;
    http = await fetch(`${URL_HTTP}${kickPath}`, { method: 'POST', body: kickBody, headers: signed('POST', kickPath, kickBody) });
    assert.equal(http.status, 200, 'expulsar assinado passa');
    assert.deepEqual(await http.json(), { kicked: 1 }, 'e derruba a sessão daquela conta');

    await espera(300);
    assert.ok(assistindo.events.some(evento => evento.event === 'kicked'), 'quem foi expulso fica sabendo');
    assert.ok(dono.events.some(evento => evento.event === 'peerKicked'), 'e a sala também');

    http = await fetch(`${URL_HTTP}/rooms/sala-que-nao-existe/kick`, { method: 'POST', body: kickBody, headers: signed('POST', '/rooms/sala-que-nao-existe/kick', kickBody) });
    assert.deepEqual(await http.json(), { kicked: 0 }, 'sala fora do ar não tem quem expulsar, e não é erro');

    // Quem já caiu pode ser tirado da lista por qualquer um; quem está ao vivo, não.
    reply = await dono.call('removePeer', { peerId: entrada.peerId });
    assert.equal(reply.status, 422, 'remover alguém ao vivo pelo socket é recusado — isso é do Laravel');

    assistindo.close();
    nativo.close();
    visitante.close();
    dono.close();

    console.log('SFU API check: OK');
};

run().catch(error => {
    console.error('check FAILED:', error.message);
    process.exit(1);
});
