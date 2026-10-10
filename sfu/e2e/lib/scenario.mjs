import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * O que todo cenário usa: portas que não se cruzam, a identidade de quem entra sem conta,
 * esperar por uma condição, guardar os números no relatório e decodificar de verdade o que
 * chegou.
 */

export const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'out');

/** Cada cenário tem o seu bloco de portas: o HTTP, a mídia e o RTP puro de cada worker. */
export const ports = slot => ({
    port: 3400 + slot * 10,
    mediaPort: 45000 + slot * 10,
    plainPort: 46000 + slot * 300,
    laravel: 8400 + slot,
});

export const guest = (room, name) => async () => ({ room, name, installId: name });

export const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

export const waitFor = async (condition, timeoutMs, what = 'condition', everyMs = 50) => {
    const deadline = Date.now() + timeoutMs;

    while (Date.now() < deadline) {
        const value = await condition();

        if (value) {
            return value;
        }

        await sleep(everyMs);
    }

    throw new Error(`timed out after ${timeoutMs} ms waiting for ${what}`);
};

/** Guarda os números do cenário em `e2e/out/report.json`, que o `run.mjs` resume no fim. */
export const record = (scenario, data) => {
    mkdirSync(OUT, { recursive: true });

    const file = join(OUT, 'report.json');
    let report = {};

    try {
        report = JSON.parse(readFileSync(file, 'utf8'));
    } catch {
        report = {};
    }

    report[scenario] = { at: new Date().toISOString(), ...data };
    writeFileSync(file, `${JSON.stringify(report, null, 2)}\n`);
};

/**
 * Passa o que quem assiste montou pelo ffmpeg: quantos quadros ele decodificou, de que
 * tamanho, e se reclamou de alguma coisa. É a prova de que "decodificável" não é só a conta
 * do harness.
 */
export const decode = (annexB, name) => {
    mkdirSync(OUT, { recursive: true });

    const file = join(OUT, `${name}.h264`);

    writeFileSync(file, annexB);

    const errors = execFileSync('ffmpeg', ['-hide_banner', '-v', 'error', '-i', file, '-f', 'null', '-'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    const probe = execFileSync('ffprobe', ['-v', 'error', '-count_frames', '-select_streams', 'v:0', '-show_entries', 'stream=nb_read_frames,width,height', '-of', 'json', file], { encoding: 'utf8' });
    const stream = JSON.parse(probe).streams[0];
    // O `stream` diz o tamanho do começo do arquivo; o do último quadro diz se a troca pegou.
    const last = execFileSync('ffprobe', ['-v', 'error', '-select_streams', 'v:0', '-show_entries', 'frame=width,height', '-of', 'csv=p=0', file], { encoding: 'utf8' }).trim().split('\n').at(-1).split(',');

    return { frames: Number(stream.nb_read_frames), width: stream.width, height: stream.height, lastWidth: Number(last[0]), lastHeight: Number(last[1]), errors: errors.trim() };
};

/** A CPU do SFU entre duas amostras do `SfuProcess.cpu()`, em % de um núcleo. */
export const cpuBetween = (before, after) => {
    const seconds = (after.at - before.at) / 1000;
    const percent = ms => Math.round((ms / 1000 / seconds) * 1000) / 10;

    return {
        seconds: Math.round(seconds * 10) / 10,
        nodePercent: percent(after.node - before.node),
        workersPercent: after.workers.map((ms, index) => percent(ms - (before.workers[index] ?? 0))),
        totalPercent: percent(after.node - before.node + after.workers.reduce((sum, ms, index) => sum + ms - (before.workers[index] ?? 0), 0)),
    };
};

/** As asserções de "assistir sem defeito" que valem para toda tela e câmera. */
export const cleanVideo = (summary, { width, height, fps, freezeMs = 500, fpsTolerance = 0.1 }) => {
    const problems = [];

    if (summary.decodable === 0) {
        problems.push('nenhum quadro decodificável');
    }

    if (summary.outOfOrder > 0) {
        problems.push(`${summary.outOfOrder} quadros fora de ordem`);
    }

    if (summary.maxFreezeMs > freezeMs) {
        problems.push(`imagem parada por ${summary.maxFreezeMs} ms`);
    }

    if (width && summary.resolution !== `${width}x${height}`) {
        problems.push(`resolução ${summary.resolution}, pedida ${width}x${height}`);
    }

    if (fps && Math.abs(summary.fps - fps) > fps * fpsTolerance) {
        problems.push(`${summary.fps} fps, pedidos ${fps}`);
    }

    if (summary.mismatch) {
        problems.push(summary.mismatch);
    }

    return problems;
};
