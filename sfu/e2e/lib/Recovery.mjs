import { extendSequence } from './rtp.mjs';

const RETRY_FLOOR_MS = 40;
const RETRY_CEILING_MS = 500;
const MOST_ASKS = 3;
const GIVE_UP_FLOOR_MS = 250;
const GIVE_UP_CEILING_MS = 1000;
const MOST_HELD = 4096;
const JUMP = 3000;

/**
 * O `recovery.rs` do app, linha a linha: segura quem chegou adiantado, pede de novo o que
 * faltou (até três vezes, no ritmo da ida e volta medida) e, passado o prazo, larga o
 * buraco e pede quadro-chave. O que sai daqui sai em ordem.
 */
export class Recovery {
    constructor() {
        this.next = null;
        this.held = new Map();
        this.missing = new Map();
        this.counters = { received: 0, recovered: 0, lost: 0, late: 0, duplicate: 0 };
        this.roundTrip = null;
        this.lateSince = null;
    }

    /** Devolve `[{ packet, gap }]`: `gap` diz que um buraco foi largado logo antes dele. */
    arrive(sequence16, packet, now) {
        this.counters.received += 1;

        if (this.next === null) {
            this.next = extendSequence(sequence16, null) + 1;

            return [{ packet, gap: false }];
        }

        const sequence = extendSequence(sequence16, this.next);

        if (sequence < this.next && this.next - sequence <= JUMP) {
            this.counters[this.lateSince !== null && sequence >= this.lateSince ? 'late' : 'duplicate'] += 1;

            return [];
        }

        if (Math.abs(sequence - this.next) > JUMP) {
            this.held.clear();
            this.missing.clear();
            this.next = sequence + 1;

            return [{ packet, gap: true }];
        }

        const ask = this.missing.get(sequence);

        if (ask) {
            this.missing.delete(sequence);
            this.counters.recovered += 1;

            if (ask.first !== null) {
                this.measure(now - ask.first);
            }
        }

        if (sequence > this.next) {
            for (let gap = this.next; gap < sequence; gap += 1) {
                if (!this.held.has(gap) && !this.missing.has(gap)) {
                    this.missing.set(gap, { since: now, asked: 0, first: null, last: null });
                }
            }

            this.held.set(sequence, packet);

            return [];
        }

        const released = [{ packet, gap: false }];

        this.next += 1;
        this.releaseReady(released);

        return released;
    }

    due(now) {
        const due = { nack: [], pli: false, released: [] };
        const retry = this.retry();
        const giveUp = this.giveUp();
        const oldest = this.missing.size > 0 ? Math.min(...this.missing.keys()) : null;
        const stale = oldest !== null && now - this.missing.get(oldest).since >= giveUp;

        if ((stale || this.held.size > MOST_HELD) && this.held.size > 0) {
            const firstHeld = Math.min(...this.held.keys());

            this.counters.lost += firstHeld - this.next;
            this.lateSince ??= this.next;

            for (const sequence of [...this.missing.keys()]) {
                if (sequence <= firstHeld) {
                    this.missing.delete(sequence);
                }
            }

            this.next = firstHeld;
            this.releaseReady(due.released, true);
            due.pli = true;
        }

        for (const [sequence, ask] of this.missing) {
            const waited = ask.last === null || now - ask.last >= retry;

            if (ask.asked < MOST_ASKS && waited) {
                ask.asked += 1;
                ask.first ??= now;
                ask.last = now;
                due.nack.push(sequence % 65536);
            }
        }

        return due;
    }

    measure(sample) {
        this.roundTrip = this.roundTrip === null ? sample : (this.roundTrip * 7 + sample) / 8;
    }

    retry() {
        if (this.roundTrip === null) {
            return RETRY_FLOOR_MS;
        }

        return Math.min(Math.max(this.roundTrip * 1.25, RETRY_FLOOR_MS), RETRY_CEILING_MS);
    }

    giveUp() {
        return Math.min(Math.max(this.retry() * MOST_ASKS + (this.roundTrip ?? 0), GIVE_UP_FLOOR_MS), GIVE_UP_CEILING_MS);
    }

    releaseReady(released, gap = false) {
        let first = gap;

        while (this.held.has(this.next)) {
            released.push({ packet: this.held.get(this.next), gap: first });
            this.held.delete(this.next);
            this.missing.delete(this.next);
            this.next += 1;
            first = false;
        }
    }
}
