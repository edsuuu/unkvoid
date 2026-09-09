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

const chamadas = [];
const argumentos = new Map();

// `broadcast.js` lê a ponte do Tauri ao carregar, então ela precisa existir antes.
globalThis.window = {
    __TAURI__: {
        core: {
            invoke: async (comando, args) => {
                chamadas.push(comando);
                argumentos.set(comando, args);

                if (comando === 'sfu_offer') {
                    return { ssrc: 7, payloadType: 96 };
                }

                return comando === 'stop_broadcast' ? 4242 : null;
            },
        },
    },
};

const { Broadcast } = await import('./ui/broadcast.js');

const pedidos = [];
const sfu = {
    request: async (acao, dados) => {
    pedidos.push(`${acao}:${dados.kind ?? dados.producerId ?? ''}/${dados.source ?? ''}`);

    return {
        producerId: dados.kind ? `${dados.kind}-producer` : undefined,
        ip: '10.0.0.1',
        port: 41000,
    };
    },
};

const transmissao = new Broadcast(sfu);

await transmissao.start('1080', 30, 'window:87');

assert.deepEqual(chamadas, ['start_broadcast', 'sfu_offer', 'sfu_offer', 'use_sfu']);
assert.equal(chamadas.indexOf('use_sfu'), chamadas.length - 1, 'use_sfu é o último');

// Qualidade e fps são escolha de quem transmite e precisam chegar inteiros ao Rust: o
// encoder e a captura são configurados com eles, e um `undefined` aqui vira 1 fps lá.
assert.deepEqual(argumentos.get('start_broadcast'), {
    quality: '1080',
    fps: 30,
    source: 'window:87',
});

// Vídeo primeiro, e os dois declarados — o áudio da tela ia junto e era esquecido.
assert.deepEqual(pedidos, ['producePlain:video/screen', 'producePlain:audio/screenAudio']);
assert.equal(transmissao.broadcasting, true);

assert.equal(await transmissao.stop(), 4242, 'stop devolve os quadros transmitidos');
assert.equal(transmissao.broadcasting, false);
assert.deepEqual(pedidos, [
    'producePlain:video/screen',
    'producePlain:audio/screenAudio',
    'closeProducer:video-producer/',
    'closeProducer:audio-producer/',
]);

// Parar duas vezes não pode mandar um segundo `stop_broadcast`: o Rust responde erro e
// a mensagem de encerramento viraria uma falha na cara de quem só clicou uma vez.
const antes = chamadas.length;

assert.equal(await transmissao.stop(), 0);
assert.equal(chamadas.length, antes, 'parar de novo não fala com o Rust');

// Uma falha depois de iniciar a captura também precisa liberar o estado nativo, para
// que a próxima tentativa não receba "a stream is already in progress".
const falhaSfu = {
    request: async (acao, dados) => {
        if (acao === 'producePlain' && dados.kind === 'audio') {
            throw new Error('SFU indisponível');
        }

        return { producerId: 'partial-producer', ip: '10.0.0.1', port: 41000 };
    },
};
const parcial = new Broadcast(falhaSfu);
await assert.rejects(() => parcial.start('1080', 30, 'display:1'), /SFU indisponível/);
assert.equal(parcial.nativeActive, false);
assert.deepEqual(parcial.producerIds, []);

console.log('transmissão: ok — ordem e encerramento');
