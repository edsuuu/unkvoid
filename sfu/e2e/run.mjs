/**
 * `pnpm run e2e [letras]`: roda os cenários (todos, ou só os das letras pedidas, ex.
 * `pnpm run e2e a c`) um de cada vez, cada um com o seu SFU, e no fim resume os números
 * que eles guardaram em `e2e/out/report.json`.
 */
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const SCENARIOS = join(HERE, 'scenarios');
const REPORT = join(HERE, 'out', 'report.json');
const wanted = process.argv.slice(2).filter(argument => argument !== '--');
const files = readdirSync(SCENARIOS)
    .filter(file => file.endsWith('.test.mjs'))
    .filter(file => wanted.length === 0 || wanted.some(letter => file.startsWith(`${letter}-`)))
    .sort()
    .map(file => join(SCENARIOS, file));

if (files.length === 0) {
    console.error(`nenhum cenário com as letras: ${wanted.join(' ')}`);
    process.exit(1);
}

if (wanted.length === 0) {
    rmSync(REPORT, { force: true });
}

const run = spawnSync(process.execPath, ['--test', '--test-concurrency=1', '--test-reporter=spec', ...files], { stdio: 'inherit' });

let report = {};

try {
    report = JSON.parse(readFileSync(REPORT, 'utf8'));
} catch {
    report = {};
}

const line = (label, value) => console.log(`  ${label.padEnd(44)} ${value}`);
const videoOf = (summaries = [], suffix) => summaries.filter(summary => summary.label?.endsWith(suffix));

console.log('\nResumo (e2e/out/report.json)\n');

if (report.a) {
    const screens = videoOf(report.a.watched, ':screen');

    line('a. tela 1080p60: fps / pior parada', `${screens.map(summary => summary.fps).join(', ')} fps / ${Math.max(...screens.map(summary => summary.maxFreezeMs))} ms`);
    line('a. quadros pulados / fora de ordem', `${report.a.watched.reduce((total, summary) => total + (summary.skipped ?? 0) + (summary.lost ?? 0), 0)} / ${report.a.watched.reduce((total, summary) => total + summary.outOfOrder, 0)}`);
}

if (report.b) {
    const worst = Math.max(...report.b.joins.flatMap(join => [join.screen, join.camera]));

    line('b. 1º quadro de quem entra atrasado (pior)', `${worst} ms (${report.b.joins.length} entradas)`);
}

if (report['b-custo']) {
    line('b. quadros-chave por atrasado (tela/câmera)', report['b-custo'].costs.map(cost => `${cost.screenKeyframes}/${cost.cameraKeyframes}`).join(' '));
}

for (const key of ['c-subida', 'c-descida', 'c-dois-lados', 'c-app-de-hoje']) {
    if (report[key]) {
        const screen = videoOf(report[key].watched, ':screen')[0];
        const counters = report[key].recovery.find(counter => counter.label.endsWith(':screen'));

        line(`${key}: pior parada / quadros / buracos`, `${screen.maxFreezeMs} ms / ${screen.decodable} / ${counters.lost} (recuperados ${counters.recovered})`);
    }
}

if (report['d-trocar-tela']) {
    line('d. 1º quadro da tela trocada', `${report['d-trocar-tela'].firstFrameMs} ms`);
}

if (report.e) {
    line('e. mover: origem perde / destino vê', `${report.e.lostAtOriginMs} ms / ${report.e.firstFrameAtDestinationMs} ms`);
}

if (report['f-worker']) {
    line('f. worker morto: quem assistia volta a ver em', `${report['f-worker'].backToWatchingMs} ms`);
}

for (const key of ['g-assiste', 'g-transmite']) {
    if (report[key]) {
        line(`${key}: imagem de volta depois da rede`, `${report[key].backAfterMs} ms`);
    }
}

if (report['g-ciclos']) {
    line(`g. ${report['g-ciclos'].cycles} ciclos: memória do 1º quarto ao fim`, `+${Math.round(report['g-ciclos'].growthKb / 1024)} MB`);
}

for (const key of ['h-um-router', 'h-dois-routers']) {
    if (report[key]) {
        line(`${key}: CPU do SFU (node + workers)`, `${report[key].cpu.totalPercent}% de um núcleo (workers ${report[key].cpu.workersPercent.join(' / ')}%)`);
    }
}

process.exit(run.status ?? 1);
