import type { Resource } from '../../types.js';

export class StatusResource implements Resource {
    constructor(private readonly status: string = 'ok') {}

    toArray(): Record<string, unknown> {
        return { status: this.status };
    }
}
