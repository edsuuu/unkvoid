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

// `broadcast.js` lê a ponte do Tauri ao carregar, então ela precisa existir antes.
globalThis.window = {
    __TAURI__: {
        core: {
            invoke: async (comando, args) => {
                chamadas.push(comando);

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
        pedidos.push(`${acao}:${dados.kind}/${dados.source}`);

        return { ip: '10.0.0.1', port: 41000 };
    },
};

const transmissao = new Broadcast(sfu);

await transmissao.start('1080', 'window:87');

assert.deepEqual(chamadas, ['start_broadcast', 'sfu_offer', 'sfu_offer', 'use_sfu']);
assert.equal(chamadas.indexOf('use_sfu'), chamadas.length - 1, 'use_sfu é o último');

// Vídeo primeiro, e os dois declarados — o áudio da tela ia junto e era esquecido.
assert.deepEqual(pedidos, ['producePlain:video/screen', 'producePlain:audio/screenAudio']);
assert.equal(transmissao.broadcasting, true);

assert.equal(await transmissao.stop(), 4242, 'stop devolve os quadros transmitidos');
assert.equal(transmissao.broadcasting, false);

// Parar duas vezes não pode mandar um segundo `stop_broadcast`: o Rust responde erro e
// a mensagem de encerramento viraria uma falha na cara de quem só clicou uma vez.
const antes = chamadas.length;

assert.equal(await transmissao.stop(), 0);
assert.equal(chamadas.length, antes, 'parar de novo não fala com o Rust');

console.log('transmissão: ok — ordem e encerramento');
