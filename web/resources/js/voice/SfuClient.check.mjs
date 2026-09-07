/**
 * Checagem executável: node resources/js/voice/SfuClient.check.mjs
 *
 * O mediasoup-client escolhe a implementação de WebRTC farejando o user-agent, e o
 * WKWebView do app não põe o token `Safari` no dele. A voz estourava com "device not
 * supported" dentro do app e funcionava no navegador — o tipo de bug que só aparece
 * onde ninguém testa.
 */
import assert from 'node:assert/strict';

const agentes = {
    'WKWebView do Tauri (sem o token Safari)':
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)',
    'Safari de verdade':
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15',
    'Chrome':
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36',
    'WebView2 do Windows':
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36 Edg/120.0',
    'WebKitGTK do Linux':
        'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0',
};

// Node expõe `navigator` como getter, então tem que ser redefinido.
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: '' }, configurable: true, writable: true });

const { SfuClient } = await import('./SfuClient.js');

const escolha = agente => {
    globalThis.navigator.userAgent = agente;

    return SfuClient.handler().handlerName ?? '(detecção automática)';
};

// Onde falta o token Safari, o handler tem que ser nomeado à mão.
assert.equal(escolha(agentes['WKWebView do Tauri (sem o token Safari)']), 'Safari12');
assert.equal(escolha(agentes['WebKitGTK do Linux']), 'Safari12');

// Onde a detecção do mediasoup funciona, não opinamos.
assert.equal(escolha(agentes['Safari de verdade']), '(detecção automática)');
assert.equal(escolha(agentes.Chrome), '(detecção automática)');
assert.equal(escolha(agentes['WebView2 do Windows']), '(detecção automática)');

console.log('SfuClient.handler: ok');
