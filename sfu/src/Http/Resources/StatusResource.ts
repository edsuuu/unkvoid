import type { Resource } from '../../types.js';

export class StatusResource implements Resource {
    public constructor(private readonly status: string = 'ok') {}

    public toArray(): Record<string, unknown> {
        return { status: this.status };
    }
}
