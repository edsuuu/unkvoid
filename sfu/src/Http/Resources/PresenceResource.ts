import type { Resource } from '../../types.js';

export class PresenceResource implements Resource {
    constructor(private readonly channels: Record<string, unknown>) {}

    toArray(): Record<string, unknown> {
        return { channels: this.channels };
    }
}
