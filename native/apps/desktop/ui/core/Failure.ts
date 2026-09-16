export class Failure {
    static message(failure: unknown): string {
        const message = (failure as { message?: unknown } | null | undefined)?.message;

        return typeof message === 'string' ? message : String(failure);
    }

    static status(failure: unknown): number | null {
        const status = (failure as { status?: unknown } | null | undefined)?.status;

        return typeof status === 'number' ? status : null;
    }
}
