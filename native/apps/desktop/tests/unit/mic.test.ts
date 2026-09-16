import { describe, expect, it } from 'vitest';

import { Mic } from '../../ui/core/Mic.ts';

function tone(amplitude: number, size = Mic.FFT_SIZE): Float32Array {
    return Float32Array.from({ length: size }, (unused, index) => amplitude * Math.sin((2 * Math.PI * index) / 64));
}

describe('detecção de voz: o nível que decide se o microfone abre', () => {
    it('silêncio fica no chão e volume cheio no teto', () => {
        expect(Mic.levelOf(new Float32Array(Mic.FFT_SIZE))).toBe(0);
        expect(Mic.levelOf(tone(1)), 'seno de amplitude 1 é o mais alto que a placa entrega').toBeGreaterThan(95);
    });

    it('sobe junto com o volume, sem passar do teto nem furar o chão', () => {
        const quiet = Mic.levelOf(tone(0.01));
        const talking = Mic.levelOf(tone(0.2));
        const shouting = Mic.levelOf(tone(0.9));

        expect(quiet).toBeLessThan(talking);
        expect(talking).toBeLessThan(shouting);
        expect(quiet).toBeGreaterThanOrEqual(0);
        expect(shouting).toBeLessThanOrEqual(100);
    });

    it('sussurro fica abaixo do padrão de sensibilidade e fala normal fica acima', () => {
        const DEFAULT_SENSITIVITY = 35;

        expect(Mic.levelOf(tone(0.001)), 'ruído de fundo não pode abrir o microfone').toBeLessThan(DEFAULT_SENSITIVITY);
        expect(Mic.levelOf(tone(0.15)), 'fala normal precisa abrir o microfone').toBeGreaterThan(DEFAULT_SENSITIVITY);
    });
});
