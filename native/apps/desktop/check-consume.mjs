/**
 * O lado de quem assiste, conferido sem abrir o app.
 *
 * Três coisas que já quebraram a sala e não aparecem em nenhum log do servidor:
 * o dono do consumer, a lista de quem está transmitindo, e o que pausar quando
 * alguém não quer gastar máquina assistindo.
 *
 * node check-consume.mjs
 */
import assert from 'node:assert/strict';

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
        kind: 'video',
        rtpParameters: {},
        peerId: 'ana',
        name: 'Ana',
        source: 'screen',
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

console.log('quem assiste: ok — dono, pausa e lista de transmissões');
