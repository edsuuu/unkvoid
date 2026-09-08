/**
 * Os ícones de estado são compartilhados entre a web e o app de propósito: foi pedido
 * que os dois desenhos fossem iguais. Este arquivo existe para que continuem.
 *
 * node icons.check.mjs
 */
import assert from 'node:assert/strict';

import { HEAD_OFF, MIC_OFF, stateBadges } from './icons.js';

// Quem está ouvindo e falando não ganha marcador nenhum: ícone que aparece sempre não
// informa nada.
assert.equal(stateBadges({ muted: false, deafened: false }), '');

const mudo = stateBadges({ muted: true, deafened: false });

assert.ok(mudo.includes(MIC_OFF), 'microfone mudo tem de mostrar o microfone cortado');
assert.ok(! mudo.includes(HEAD_OFF), 'microfone mudo não é fone mudo');

const surdo = stateBadges({ muted: false, deafened: true });

assert.ok(surdo.includes(HEAD_OFF), 'áudio mudo tem de mostrar o fone cortado');

// Surdo no app fecha o microfone junto, então os dois aparecem — e nessa ordem, que é
// a mesma do Discord e a mesma dos botões da barra de voz.
const ambos = stateBadges({ muted: true, deafened: true });

assert.ok(ambos.indexOf(MIC_OFF) < ambos.indexOf(HEAD_OFF), 'microfone vem antes do fone');

// O vermelho é o mesmo dos dois lados. Divergir aqui é justamente o que se quis evitar.
assert.ok(ambos.includes('text-[#f23f43]'), 'o marcador é vermelho');

console.log('ícones de estado: ok');
