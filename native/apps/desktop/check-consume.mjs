/**
 * O lado de quem assiste, conferido sem abrir o app.
 *
 * Três coisas que já quebraram a sala e não aparecem em nenhum log do servidor:
 * o dono do consumer, a lista de quem está transmitindo, e o que pausar quando
 * alguém não quer gastar máquina assistindo. E, com o DOM de verdade, o que o `App`
 * faz com cada origem: áudio da tela chega mudo, mic toca, câmera vira cartão.
 *
 * node check-consume.mjs
 */
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

import { JSDOM } from 'jsdom';

const { SfuClient } = await import('./ui/SfuClient.js');

const client = new SfuClient();
const requests = [];

client.device = { rtpCapabilities: {} };
client.recvTransport = {
    id: 'recv-1',
    consume: async params => ({ ...params }),
};
client.request = async (action, data) => {
    requests.push(`${action}:${data.consumerId ?? data.producerId ?? ''}`);

    return {
        consumerId: `consumer-de-${data.producerId}`,
        producerId: data.producerId,
        kind: data.producerId?.startsWith('mic') ? 'audio' : 'video',
        rtpParameters: {},
        peerId: 'ana',
        name: 'Ana',
        source: data.producerId?.startsWith('mic') ? 'mic' : 'screen',
    };
};

// O dono vem da resposta do servidor. Referenciar um `peerId` que ninguém declarou é
// ReferenceError em módulo: o vídeo chegava, a exceção subia, e a tela nunca aparecia.
const { consumer, peerId } = await client.consume('video-producer');

assert.equal(peerId, 'ana');
assert.deepEqual(client.consumersOf('ana'), [consumer.id]);
assert.deepEqual(client.consumersOf('bruno'), []);

// Pausar fala com o servidor: parar só o <video> continuaria baixando e decodificando.
requests.length = 0;
await client.setPeerPaused('ana', true);
assert.deepEqual(requests, [`pauseConsumer:${consumer.id}`]);

await client.setPeerPaused('ana', false);
assert.deepEqual(requests.at(-1), `resumeConsumer:${consumer.id}`);

// Pausar a tela de alguém não pode calar o microfone dela: sem o filtro por `kind`, o
// `pauseConsumer` levava tela, áudio da tela, mic e câmera juntos.
const { consumer: mic } = await client.consume('mic-producer');

assert.deepEqual(client.consumersOf('ana', 'audio'), [mic.id]);
assert.equal(client.consumersOf('ana').length, 2, 'sem `kind`, a lista continua inteira');
requests.length = 0;
await client.setPeerPaused('ana', true, 'video');
assert.deepEqual(requests, [`pauseConsumer:${consumer.id}`], 'o mic segue tocando');

// A lista de producers é o que permite clicar em "assistir" depois. Sem ela, quem
// perdeu o instante do `newProducer` só via a tela saindo e entrando da sala.
client.peers.clear();
client.trackPeers('peerJoined', { peerId: 'ana', name: 'Ana' });
client.trackPeers('newProducer', { peerId: 'ana', producerId: 'v1', kind: 'video', source: 'screen' });
client.trackPeers('newProducer', { peerId: 'ana', producerId: 'a1', kind: 'audio', source: 'screenAudio' });

assert.equal(client.peers.get('ana').sharing, true);
assert.deepEqual(client.peers.get('ana').producers.map(item => item.producerId), ['v1', 'a1']);

// O mesmo `newProducer` chegando duas vezes não pode duplicar o item: consumir duas
// vezes o mesmo producer devolve erro do servidor.
client.trackPeers('newProducer', { peerId: 'ana', producerId: 'v1', kind: 'video', source: 'screen' });
assert.equal(client.peers.get('ana').producers.length, 2);

// Parar a transmissão apaga o vídeo e o "está compartilhando", mas o áudio segue.
client.trackPeers('producerClosed', { peerId: 'ana', producerId: 'v1', kind: 'video', source: 'screen' });
assert.equal(client.peers.get('ana').sharing, false);
assert.deepEqual(client.peers.get('ana').producers.map(item => item.producerId), ['a1']);

// `peersChanged` só quando a lista muda: um `consumerClosed` não redesenha ninguém.
const changes = [];

client.addEventListener('peersChanged', () => changes.push(1));
client.trackPeers('consumerClosed', { consumerId: 'x' });
client.trackPeers('producerPaused', { peerId: 'ana', producerId: 'a1' });
assert.equal(changes.length, 1);
assert.equal(client.peers.get('ana').producers[0].paused, true);

// ---- O `App` com o DOM de verdade: o que cada origem vira na tela.
const dom = new JSDOM(await readFile('./ui/index.html', 'utf8'), { url: 'http://localhost/' });
const { window } = dom;

// Os módulos leem a ponte do Tauri ao carregar; sem console na janela, cada `invoke`
// vira uma promessa resolvida e o log fica na memória.
window.__TAURI__ = { core: { invoke: async () => null }, event: { listen: async () => null } };
window.HTMLMediaElement.prototype.play = async () => null;

globalThis.window = window;
globalThis.document = window.document;

// O jsdom nasce em `prerender`, que o app lê como janela oculta: sem isto todo cartão
// chegaria pausado.
Object.defineProperty(window.document, 'hidden', { value: false, configurable: true });
globalThis.localStorage = window.localStorage;
globalThis.sessionStorage = window.sessionStorage;
globalThis.MediaStream = class { constructor(tracks = []) { this.tracks = tracks; } getTracks() { return this.tracks; } };
globalThis.CSS = window.CSS;
globalThis.location = window.location;
Object.defineProperty(globalThis, 'navigator', { value: window.navigator, configurable: true });

const { App } = await import('./ui/app.js');
const app = new App();
const track = { stop() {} };

app.sfu = {
    canWatch: () => true,
    consumersHasProducer: () => false,
    peers: new Map([['ana', { peerId: 'ana', name: 'Ana' }]]),
    consume: async producerId => ({
        consumer: { track, kind: producerId.startsWith('a') ? 'audio' : 'video' },
        peerId: 'ana',
        source: { 'a-screen': 'screenAudio', 'a-mic': 'mic', 'v-camera': 'camera', 'v-screen': 'screen' }[producerId],
    }),
};

await app.consume({ producerId: 'a-screen', peerId: 'ana', kind: 'audio', source: 'screenAudio' });
await app.consume({ producerId: 'a-mic', peerId: 'ana', kind: 'audio', source: 'mic' });
await app.consume({ producerId: 'v-camera', peerId: 'ana', kind: 'video', source: 'camera' });
await app.consume({ producerId: 'v-screen', peerId: 'ana', kind: 'video', source: 'screen' });

// O som da tela de alguém invadindo a sala sem aviso é pior do que um clique para
// ligar; o mic é o contrário: ninguém entra numa chamada para não ouvir.
assert.equal(app.remoteAudios.get('ana').muted, true, 'áudio da tela chega mudo');
assert.equal(app.micAudios.get('a-mic').muted, false, 'mic toca direto');
assert.equal(document.querySelector('[data-screen="ana/camera"]').dataset.kind, 'camera', 'câmera vira cartão pequeno');

// O ajuste de imagem grava a variável E marca o vídeo: sem `data-tuned` o CSS não
// aplica filtro nenhum, e o vídeo fica no caminho direto do compositor.
const video = document.querySelector('[data-screen="ana"] video');
const contrast = document.querySelector('[data-screen="ana"] [data-contrast]');

assert.ok(! video.hasAttribute('data-tuned'), 'sem mexer, sem filtro');
contrast.value = '150';
contrast.dispatchEvent(new window.Event('input'));
assert.equal(video.style.getPropertyValue('--contrast'), '1.5');
assert.ok(video.hasAttribute('data-tuned'));
document.querySelector('[data-screen="ana"] [data-video-reset]').click();
assert.ok(! video.hasAttribute('data-tuned'), 'voltar ao padrão tira o filtro');

// O áudio da tela fechou do outro lado: o `<audio>` sai junto, o mic fica.
app.forgetProducer({ producerId: 'a-screen', peerId: 'ana', kind: 'audio', source: 'screenAudio' });
assert.equal(app.remoteAudios.has('ana'), false);
assert.equal(document.querySelectorAll('audio[data-remote="ana"]').length, 1);

// ---- O teto de telas que abrem sozinhas: cada uma é um decoder de H.264 inteiro, e em
// quatro núcleos quatro cartões ao vivo custam o jogo. A tela da Ana já ocupa um lugar.
assert.ok([2, 4].includes(App.MAX_SCREENS));

const crowd = ['bia', 'caio'].map(peerId => ({ peerId, name: peerId, producers: [{ producerId: `v-${peerId}`, kind: 'video', source: 'screen' }] }));

crowd.forEach(peer => app.sfu.peers.set(peer.peerId, peer));
app.sfu.consume = async producerId => ({ consumer: { track, kind: 'video' }, peerId: producerId.replace('v-', ''), source: 'screen' });
await app.consumePeers(crowd, 2);
assert.ok(document.querySelector('[data-screen="bia"]'), 'a segunda tela ainda cabe no teto');
assert.ok(! document.querySelector('[data-screen="caio"]'), 'acima do teto, o cartão espera o Assistir');

// O "Assistir" passa por cima do teto: ali quem clicou já decidiu pagar o cartão a mais.
await app.watchPeer('caio');
assert.ok(document.querySelector('[data-screen="caio"]'), 'clicar em Assistir sempre consome');

// ---- Sem encoder na placa a transmissão sai pelo processador, e quem transmite precisa
// saber disso uma vez — não a cada leitura de um segundo.
const toasts = [];
const cpuReading = { active: true, captured: 0, sent: 0, sentBytes: 0, sendDropped: 0, encodeErrors: 0, sendErrors: 0, audioErrors: 0, busyUs: 0, encoder: 'cpu' };

app.toast = message => toasts.push(message);
app.updateBroadcastStats({ ...cpuReading, encoder: 'gpu' });
assert.equal(toasts.length, 0, 'encoder da placa não avisa nada');
app.updateBroadcastStats(cpuReading);
app.updateBroadcastStats(cpuReading);
assert.equal(toasts.length, 1, 'encoder do processador avisa uma vez');
assert.match(toasts[0], /processador/);

for (const timer of app.mediaStatsTimers.values()) {
    clearInterval(timer);
}

console.log('quem assiste: ok — dono, pausa por kind, teto de telas, aviso de CPU e o que cada origem vira na tela');
