import type { Resource } from '../../types.js';

export class PongResource implements Resource {
    public toArray(): Record<string, unknown> {
        return {};
    }
}
