/**
 * O `BitrateGovernor` do app (`media/src/governor.rs`), constante por constante: a taxa desce
 * 30% numa janela de 1 s com perda de 5% ou mais, espera 8 janelas para decidir de novo, e sobe
 * 5% a cada 5 janelas limpas até o teto.
 */
const MIN_PACKETS = 100;
const CONGESTED_PERMILLE = 50;
const CLEAN_PERMILLE = 10;
const CLEAN_WINDOWS = 5;
const HOLDOFF_WINDOWS = 8;
const DECREASE_PERCENT = 70;
const INCREASE_PERCENT = 105;
const FLOOR_PERCENT = 35;

export class Governor {
    constructor(ceiling) {
        this.ceiling = ceiling;
        this.floor = Math.floor((ceiling * FLOOR_PERCENT) / 100);
        this.target = ceiling;
        this.clean = 0;
        this.holdoff = 0;
        this.sent = 0;
        this.nacked = 0;
        this.history = [];
    }

    observe(sent, nacked, now = Date.now()) {
        this.sent += sent;
        this.nacked += nacked;

        if (this.sent < MIN_PACKETS) {
            return;
        }

        const loss = Math.min(Math.floor((this.nacked * 1000) / this.sent), 1000);

        this.sent = 0;
        this.nacked = 0;

        if (this.holdoff > 0) {
            this.holdoff -= 1;

            return;
        }

        let wanted = this.target;

        if (loss >= CONGESTED_PERMILLE) {
            this.clean = 0;
            wanted = Math.max(Math.floor((this.target * DECREASE_PERCENT) / 100), this.floor);
        } else if (loss < CLEAN_PERMILLE) {
            this.clean += 1;

            if (this.clean < CLEAN_WINDOWS) {
                return;
            }

            this.clean = 0;
            wanted = Math.min(Math.floor((this.target * INCREASE_PERCENT) / 100), this.ceiling);
        } else {
            this.clean = 0;

            return;
        }

        if (wanted < this.target) {
            this.holdoff = HOLDOFF_WINDOWS;
        }

        if (wanted !== this.target) {
            this.history.push({ at: now, from: this.target, to: wanted, loss });
        }

        this.target = wanted;
    }

    /** A subida já mostrou que não leva o teto: o freio de quadro-chave do app não faz rajada. */
    constrained() {
        return this.target < this.ceiling;
    }
}
