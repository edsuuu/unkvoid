import { describe, expect, it } from 'vitest';

import { RoomCode } from '../../ui/core/RoomCode.ts';

const SAMPLE_SIZE = 20_000;

describe('código de sala: a única chave que existe, então sorteio torto é sala adivinhável', () => {
    const codes = Array.from({ length: SAMPLE_SIZE }, () => RoomCode.generate());

    it('todo código sorteado tem o tamanho e o formato que o servidor aceita', () => {
        for (const code of codes) {
            expect(code.length, `código com tamanho errado: ${code}`).toBe(RoomCode.LENGTH);
            expect(RoomCode.isValid(code), `código fora do formato aceito pelo servidor: ${code}`).toBe(true);
        }
    });

    it('aceita nome legível e o mínimo; recusa vazio, curto, longo, maiúscula e hífen na ponta', () => {
        expect(RoomCode.isValid(''), 'vazio não é código').toBe(false);
        expect(RoomCode.isValid('sala-do-time'), 'nome legível precisa ser aceito').toBe(true);
        expect(RoomCode.isValid('abc'), 'código mínimo precisa ser aceito').toBe(true);
        expect(RoomCode.isValid('ab'), 'curto demais não é código').toBe(false);
        expect(RoomCode.isValid('a'.repeat(33)), 'longo demais não é código').toBe(false);
        expect(RoomCode.isValid('A'.repeat(RoomCode.LENGTH)), 'maiúscula não é código — o servidor recusa').toBe(false);
        expect(RoomCode.isValid('sala-'), 'hífen no fim não é código').toBe(false);
    });

    it('vinte mil sorteios não repetem código', () => {
        expect(new Set(codes).size).toBe(SAMPLE_SIZE);
    });

    it('nenhuma letra sai muito mais que as outras: o alfabeto inteiro aparece perto do esperado', () => {
        const counts = new Map<string, number>();

        for (const character of codes.join('')) {
            counts.set(character, (counts.get(character) ?? 0) + 1);
        }

        const expected = (SAMPLE_SIZE * RoomCode.LENGTH) / 36;

        expect(counts.size, 'o alfabeto inteiro precisa sair no sorteio').toBe(36);

        for (const [character, times] of counts) {
            expect(Math.abs(times - expected), `"${character}" saiu ${times} vezes, esperado ~${Math.round(expected)} — sorteio enviesado`).toBeLessThan(expected * 0.15);
        }
    });
});
