/**
 * A ordem da transmissão, conferida sem abrir o app.
 *
 * O `use_sfu` tem de vir DEPOIS de declarar vídeo e áudio. Ao contrário, o Rust começa
 * a mandar RTP de um SSRC que o servidor ainda não conhece, e o servidor descarta os
 * pacotes calado: a transmissão "funciona" e ninguém vê nada. É o tipo de erro que só
 * aparece com duas pessoas de verdade, então fica registrado aqui.
 *
 * node check-broadcast.mjs
 */
import assert from 'node:assert/strict';

const calls = [];
const callArgs = new Map();

// `broadcast.js` lê a ponte do Tauri ao carregar, então ela precisa existir antes.
globalThis.window = {
    __TAURI__: {
        core: {
            invoke: async (command, args) => {
                calls.push(command);
                callArgs.set(command, args);

                if (command === 'sfu_offer') {
                    return { ssrc: 7, payloadType: 96 };
                }

                return command === 'stop_broadcast' ? 4242 : null;
            },
        },
    },
};

const { Broadcast } = await import('./ui/broadcast.js');

const requests = [];
const sfu = {
    request: async (acao, dados) => {
    requests.push(`${acao}:${dados.kind ?? dados.producerId ?? ''}/${dados.source ?? ''}`);

    return {
        producerId: dados.kind ? `${dados.kind}-producer` : undefined,
        ip: '10.0.0.1',
        port: 41000,
    };
    },
};

const broadcast = new Broadcast(sfu);

await broadcast.start('1080', 30, 'window:87', true, true);

assert.deepEqual(calls, ['start_broadcast', 'sfu_offer', 'sfu_offer', 'use_sfu']);
assert.equal(calls.indexOf('use_sfu'), calls.length - 1, 'use_sfu é o último');

// Qualidade, fps e as duas opções de áudio são escolha de quem transmite e precisam
// chegar inteiras ao Rust: o encoder e a captura são configurados com elas, e um
// `undefined` aqui vira 1 fps lá, ou o áudio da chamada do Discord na transmissão.
assert.deepEqual(callArgs.get('start_broadcast'), {
    quality: '1080',
    fps: 30,
    source: 'window:87',
    audio: true,
    muteCalls: true,
});

// Vídeo primeiro, e os dois declarados — o áudio da tela ia junto e era esquecido.
assert.deepEqual(requests, ['producePlain:video/screen', 'producePlain:audio/screenAudio']);
assert.equal(broadcast.broadcasting, true);

// O servidor reiniciou: os producers antigos morreram lá, a captura continua viva aqui.
// Republicar declara os dois de novo e reaponta o destino, sem tocar em `start_broadcast`
// — reiniciar a captura tiraria a tela do ar e pediria a permissão outra vez.
const antes = calls.length;

assert.equal(await broadcast.republish(), true);
assert.deepEqual(calls.slice(antes), ['renew_sfu_key', 'sfu_offer', 'sfu_offer', 'use_sfu']);

// Chave nova antes de qualquer oferta: a oferta carrega a chave, e pedir depois mandaria
// ao servidor a chave velha enquanto o Rust cifra com a nova.
assert.equal(calls.slice(antes)[0], 'renew_sfu_key');

// A lista não pode acumular: `stop` fecharia producers que já não existem, e o primeiro
// erro derrubaria a mensagem de encerramento.
assert.deepEqual(broadcast.producerIds, ['video-producer', 'audio-producer']);
assert.equal(broadcast.videoProducerId, 'video-producer');

assert.equal(await broadcast.stop(), 4242, 'stop devolve os quadros transmitidos');
assert.equal(broadcast.broadcasting, false);
assert.deepEqual(requests, [
    'producePlain:video/screen',
    'producePlain:audio/screenAudio',
    'producePlain:video/screen',
    'producePlain:audio/screenAudio',
    'closeProducer:video-producer/',
    'closeProducer:audio-producer/',
]);

// Republicar sem transmissão nenhuma não fala com o Rust: o `afterReconnect` chama isto
// toda vez que a sessão é nova, e quem só assiste passa por aqui.
const parado = new Broadcast(sfu);

assert.equal(await parado.republish(), false);

// Parar duas vezes não pode mandar um segundo `stop_broadcast`: o Rust responde erro e
// a mensagem de encerramento viraria uma falha na cara de quem só clicou uma vez.
const before = calls.length;

assert.equal(await broadcast.stop(), 0);
assert.equal(calls.length, before, 'parar de novo não fala com o Rust');

// Uma falha depois de iniciar a captura também precisa liberar o estado nativo, para
// que a próxima tentativa não receba "a stream is already in progress".
const failingSfu = {
    request: async (acao, dados) => {
        if (acao === 'producePlain' && dados.kind === 'audio') {
            throw new Error('SFU indisponível');
        }

        return { producerId: 'partial-producer', ip: '10.0.0.1', port: 41000 };
    },
};
const partial = new Broadcast(failingSfu);
await assert.rejects(() => partial.start('1080', 30, 'display:1', true, false), /SFU indisponível/);
assert.equal(partial.nativeActive, false);
assert.deepEqual(partial.producerIds, []);

console.log('transmissão: ok — ordem e encerramento');
