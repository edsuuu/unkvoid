/**
 * O relógio da chamada, conferido de fora.
 *
 * Ele já mostrou "64:12" numa call de uma hora — minuto que passa de 59 sem virar hora.
 * O `clock` é lido do próprio `app.js` em vez de copiado: assim este arquivo quebra se
 * a função sumir num "substitui esse bloco", que é como ela morreria de verdade.
 *
 * node check-clock.mjs
 */
import { readFileSync } from 'node:fs';

const fonte = readFileSync(new URL('./ui/app.js', import.meta.url), 'utf8');
const trecho = fonte.match(/const clock = seconds => \{[\s\S]*?\n\};/);

if (! trecho) {
    console.error('FALHA: `clock` sumiu de ui/app.js');
    process.exit(1);
}

const clock = eval(`${trecho[0].replace('const clock =', '')}`);

const casos = [
    [0, '00:00'],
    [9, '00:09'],
    [59, '00:59'],
    [64, '01:04'],
    [3599, '59:59'],
    // A hora aparece só quando existe — e 64 minutos são 1:04:00, não "64:00".
    [3600, '1:00:00'],
    [3852, '1:04:12'],
    [37_230, '10:20:30'],
];

const falhas = casos.filter(([segundos, esperado]) => clock(segundos) !== esperado);

for (const [segundos, esperado] of falhas) {
    console.error(`FALHA: clock(${segundos}) deu ${clock(segundos)}, esperado ${esperado}`);
}

console.log(falhas.length ? '' : `relógio: ok — ${casos.length} casos`);
process.exit(falhas.length ? 1 : 0);
