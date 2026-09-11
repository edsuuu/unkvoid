/**
 * O motor da janela pode não ter WebRTC, e citar o que não existe derruba o app.
 *
 * No WebKitGTK sem WebRTC não há `RTCRtpReceiver` nem `RTCRtpSender`: citar o nome
 * cru não devolve `undefined`, levanta "can't find variable". Foi assim que o aviso
 * de H.264 faltando virou um erro ao entrar na sala em todo Linux — a exceção subia
 * do próprio diagnóstico e engolia o recado que ele existia para dar.
 *
 * `typeof X` e `globalThis.X` são as duas formas que não levantam. Qualquer outra
 * menção a um global de WebRTC na interface volta a ser a mesma armadilha.
 *
 * node check-webrtc-absent.mjs
 */
import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';

const GLOBALS = /(?<!\.)\b(RTCRtpReceiver|RTCRtpSender|RTCPeerConnection)\b/g;
const SAFE = /(typeof\s+|globalThis\.)$/;

// Comentário citando o nome é inofensivo, e esta base cita bastante. Some com eles
// trocando por espaço, para a contagem de linhas do relato continuar valendo.
// ponytail: troca textual, cega para "//" dentro de string. Se isso passar a dar
// falso positivo, o caminho é um parser de verdade (acorn) em vez de mais regex.
const stripComments = source => source
    .replace(/\/\*[\s\S]*?\*\//g, blank => blank.replace(/[^\n]/g, ' '))
    .replace(/(^|[^:])\/\/[^\n]*/g, (whole, before) => before + ' '.repeat(whole.length - before.length));

const files = (await readdir('./ui')).filter(name => name.endsWith('.js'));
const bare = [];

for (const name of files) {
    const source = stripComments(await readFile(`./ui/${name}`, 'utf8'));

    for (const hit of source.matchAll(GLOBALS)) {
        if (SAFE.test(source.slice(0, hit.index))) {
            continue;
        }

        bare.push(`${name}:${source.slice(0, hit.index).split('\n').length} ${hit[1]}`);
    }
}

assert.deepEqual(bare, [], `global de WebRTC citado cru (use typeof ou globalThis.): ${bare.join(', ')}`);

// E a forma segura devolve nulo em vez de levantar, que é o ponto de usá-la.
assert.equal(globalThis.RTCRtpReceiver?.getCapabilities?.('video')?.codecs ?? null, null);

console.log('webrtc ausente: ok — nenhum global de WebRTC citado cru na interface');
