/**
 * Prova que um socket mudo é derrubado pelo servidor.
 *
 * Um cliente que não responde ao ping é indistinguível de um cliente vivo, do ponto de
 * vista do TCP, quando a queda é suja: a tampa do notebook fecha e nunca chega FIN nem
 * RST. Enquanto o servidor não perguntava, essa pessoa ficava ativa na sala para sempre,
 * o router do mediasoup nunca era devolvido e as portas de RTP puro dela também não.
 * Este arquivo existe porque esse vazamento não aparece em nenhum teste de caminho feliz.
 *
 * node check-heartbeat.mjs
 */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';

import WebSocket from 'ws';

const PORT = 3199;
const HEARTBEAT_MS = 200;

const server = spawn('node', ['dist/server.js'], {
    env: {
        ...process.env,
        SFU_PORT: String(PORT),
        SFU_SECRET: process.env.SFU_SECRET ?? 'segredo-de-teste-com-mais-de-32-caracteres',
        SFU_WORKERS: '1',
        SFU_HEARTBEAT_MS: String(HEARTBEAT_MS),
    },
    stdio: ['ignore', 'pipe', 'inherit'],
});

const ready = new Promise((resolve, reject) => {
    server.stdout.on('data', chunk => String(chunk).includes('SFU em') && resolve());
    server.on('exit', code => reject(new Error(`o servidor saiu antes de subir (${code})`)));
    setTimeout(() => reject(new Error('o servidor não subiu em 15s')), 15_000);
});

const finish = code => {
    server.kill('SIGTERM');
    process.exit(code);
};

try {
    await ready;

    // `autoPong: false` é o cliente fingindo estar morto sem fechar a conexão. É
    // exatamente o que um notebook com a tampa fechada parece, visto daqui.
    const silent = new WebSocket(`ws://127.0.0.1:${PORT}/sfu`, { autoPong: false });

    await new Promise((resolve, reject) => {
        silent.on('open', resolve);
        silent.on('error', reject);
    });

    const dropped = await new Promise(resolve => {
        silent.on('close', () => resolve(true));
        setTimeout(() => resolve(false), HEARTBEAT_MS * 15);
    });

    assert.equal(dropped, true, 'quem não responde ao ping precisa perder a conexão');

    // E quem responde continua de pé: derrubar todo mundo seria o oposto do conserto.
    const alive = new WebSocket(`ws://127.0.0.1:${PORT}/sfu`);

    await new Promise((resolve, reject) => {
        alive.on('open', resolve);
        alive.on('error', reject);
    });

    const survived = await new Promise(resolve => {
        alive.on('close', () => resolve(false));
        setTimeout(() => resolve(true), HEARTBEAT_MS * 10);
    });

    assert.equal(survived, true, 'quem responde ao ping não pode ser derrubado junto');

    console.log('heartbeat: ok — socket mudo cai, socket vivo fica');
    finish(0);
} catch (failure) {
    console.error('heartbeat FALHOU:', failure.message);
    finish(1);
}
