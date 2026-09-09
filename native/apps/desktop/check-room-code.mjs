/**
 * O código da sala é a única chave que existe. Um sorteio torto — enviesado, curto, ou
 * um laço que não sai — não aparece na tela: aparece como sala adivinhável ou app travado.
 *
 * node check-room-code.mjs
 */
import assert from 'node:assert/strict';

import { CODE_LENGTH, isRoomCode, newRoomCode } from './ui/room-code.js';

const AMOSTRA = 20_000;

const codigos = Array.from({ length: AMOSTRA }, newRoomCode);

for (const codigo of codigos) {
    assert.equal(codigo.length, CODE_LENGTH, `código com tamanho errado: ${codigo}`);
    assert.ok(isRoomCode(codigo), `código fora do formato aceito pelo servidor: ${codigo}`);
}

assert.ok(! isRoomCode(''), 'vazio não é código');
assert.ok(! isRoomCode('a'.repeat(CODE_LENGTH - 1)), 'curto demais não é código');
assert.ok(! isRoomCode('A'.repeat(CODE_LENGTH)), 'maiúscula não é código — o servidor recusa');
assert.ok(! isRoomCode(`${'a'.repeat(CODE_LENGTH - 1)}-`), 'hífen não é código');

// Repetição em vinte mil sorteios seria sorte grande demais para ser sorte.
assert.equal(new Set(codigos).size, AMOSTRA, 'dois códigos iguais em uma amostra pequena');

// Viés: com rejeição, nenhuma letra pode aparecer muito mais que as outras. O esperado
// por caractere é AMOSTRA * CODE_LENGTH / 36; 15% de folga cobre a variação normal.
const contagem = new Map();

for (const caractere of codigos.join('')) {
    contagem.set(caractere, (contagem.get(caractere) ?? 0) + 1);
}

const esperado = (AMOSTRA * CODE_LENGTH) / 36;

assert.equal(contagem.size, 36, 'o alfabeto inteiro precisa sair no sorteio');

for (const [caractere, vezes] of contagem) {
    assert.ok(
        Math.abs(vezes - esperado) < esperado * 0.15,
        `"${caractere}" saiu ${vezes} vezes, esperado ~${Math.round(esperado)} — sorteio enviesado`,
    );
}

console.log('código de sala: ok — formato, unicidade e distribuição');
