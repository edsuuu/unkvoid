const STARTING_RATE = 25_000_000;
const FLOOR = 4_000_000;
const BURST_MS = 20;
const MOST_BURST_FLOOR = 12_000;
const MOST_QUEUED = 4096;
const TICK_MS = 2;

/**
 * O balde do `pacer.rs`: o vídeo sai a 2,5× a taxa dele, o quadro-chave espalhado em vez
 * de uma rajada, e o reenvio na frente da fila, no mesmo ritmo.
 */
export class Pacer {
    constructor(send) {
        this.send = send;
        this.rate = STARTING_RATE;
        this.queue = [];
        this.repairs = [];
        this.budget = MOST_BURST_FLOOR;
        this.last = performance.now();
        this.timer = null;
        this.dropped = 0;
        this.stopped = false;
    }

    follow(videoBitrate) {
        this.rate = Math.max(videoBitrate * 2.5, FLOOR);
    }

    push(packet) {
        if (this.queue.length >= MOST_QUEUED) {
            this.queue.shift();
            this.dropped += 1;
        }

        this.queue.push(packet);
        this.drain();
    }

    pushRepair(packet) {
        // O `MOST_REPAIRS` do `pacer.rs`: cheio, o reparo mais velho sai.
        if (this.repairs.length >= 1024) {
            this.repairs.shift();
            this.dropped += 1;
        }

        this.repairs.push(packet);
        this.drain();
    }

    drain() {
        if (this.stopped) {
            return;
        }

        const now = performance.now();
        const bytesPerMs = this.rate / 8 / 1000;
        const most = Math.max(bytesPerMs * BURST_MS, MOST_BURST_FLOOR);

        this.budget = Math.min(this.budget + bytesPerMs * (now - this.last), most);
        this.last = now;

        while (this.repairs.length > 0 || this.queue.length > 0) {
            const lane = this.repairs.length > 0 ? this.repairs : this.queue;

            if (this.budget < lane[0].length) {
                break;
            }

            this.budget -= lane[0].length;
            this.send(lane.shift());
        }

        if ((this.repairs.length > 0 || this.queue.length > 0) && !this.timer) {
            this.timer = setTimeout(() => {
                this.timer = null;
                this.drain();
            }, TICK_MS);
        }
    }

    stop() {
        this.stopped = true;
        clearTimeout(this.timer);
        this.queue = [];
        this.repairs = [];
    }
}
