/**
 * O nome do parâmetro no deep link tem que ser o mesmo dos dois lados.
 *
 * Um renomeio automático já trocou `erro` por `error` só no app: a falha do Google
 * chegava e era ignorada, a tela de login ficava muda, e parecia que o botão não fazia
 * nada. Um caractere de diferença, nenhum erro no console.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const app = readFileSync(new URL('./ui/app.js', import.meta.url), 'utf8');
const servidor = readFileSync(
    new URL('../../../web/app/Http/Controllers/Auth/GoogleController.php', import.meta.url),
    'utf8',
);

const enviados = [...servidor.matchAll(/discord2:\/\/auth\?(\w+)=/g)].map(m => m[1]);
const lidos = [...app.matchAll(/params\.get\('(\w+)'\)/g)].map(m => m[1]);

assert.ok(enviados.length >= 2, `o servidor deveria mandar token e erro, mandou: ${enviados}`);

for (const nome of enviados) {
    assert.ok(lidos.includes(nome), `o servidor manda "${nome}" e o app não lê — o app lê ${lidos}`);
}

console.log('deep link: ok —', enviados.join(', '));
