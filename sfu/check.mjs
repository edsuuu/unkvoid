/**
 * O contrato do SFU, conferido contra um servidor de verdade.
 *
 * Sobe o SFU (`npm run build && SFU_CONNECTIONS_PER_MINUTE=200 node dist/server.js`) e
 * rode `npm run check`. O teto de conexões precisa ser levantado porque a conferência
 * abre uma dúzia de clientes de uma vez, que é exatamente o que ele existe para barrar.
 */
import assert from 'node:assert/strict';

const URL_WS = process.env.SFU_CHECK_URL ?? 'ws://127.0.0.1:3000/sfu';

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
    assert.equal(reply.status, 422, 'entrar sem sala nem nome é erro de validação');

    reply = await visitante.call('join', { room: 'MAIÚSCULA123', name: 'X' });
    assert.equal(reply.status, 422, 'código fora do formato deve ser recusado');

    reply = await visitante.call('join', { room, name: '' });
    assert.equal(reply.status, 422, 'nome vazio deve ser recusado');

    reply = await visitante.call('join', { room, name: 'x'.repeat(41) });
    assert.equal(reply.status, 422, 'nome longo demais deve ser recusado');

    reply = await visitante.call('createTransport', {});
    assert.equal(reply.status, 401, 'ação sem sessão deve dar 401');

    // A identidade nasce no servidor: ninguém escolhe o próprio id.
    const dono = await abrir();
    const entrada = await entrar(dono, { room, name: 'Dono' });

    assert.ok(entrada.peerId, 'o servidor devolve o id do participante');
    assert.ok(entrada.resumeKey, 'e a chave para voltar depois de uma queda');
    assert.notEqual(entrada.peerId, entrada.resumeKey, 'a chave não pode ser o id, que a sala inteira conhece');
    assert.ok(entrada.routerRtpCapabilities.codecs.length > 0, 'e as capacidades do router');
    assert.equal(entrada.resumed, false, 'a primeira entrada não é retomada');

    reply = await dono.call('join', { room, name: 'Dono' });
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
    const outraSessao = await entrar(impostor, { room, name: 'Impostor', resumeKey: entrada.peerId, resume: true });

    assert.equal(outraSessao.resumed, false, 'o peerId de outro não retoma sessão nenhuma');
    assert.notEqual(outraSessao.peerId, entrada.peerId, 'e nem rouba o id');
    impostor.close();

    // Queda de sinalização não tira ninguém da sala: voltar dentro da carência retoma
    // a sessão com a mídia intacta.
    const solucador = await abrir();
    const soluco = await entrar(solucador, { room, name: 'Soluço' });

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
    const retomada = await entrar(voltou, { room, name: 'Soluço', resumeKey: soluco.resumeKey, resume: true });

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
    const limpa = await entrar(reaberto, { room, name: 'Soluço', resumeKey: soluco.resumeKey });

    assert.equal(limpa.resumed, false, 'sem resume:true NÃO pode retomar — o cliente não tem transporte');
    reaberto.close();

    // Ingest de RTP puro: o app declara o que vai mandar antes de mandar.
    const nativo = await abrir();
    await entrar(nativo, { room, name: 'Nativo' });

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
    await entrar(assistindo, { room, name: 'Assiste' });

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
