export class Field {
    static require(value: string, message: string): void {
        if (value.trim() === '') {
            throw new Error(message);
        }
    }

    static requireEmail(value: string): void {
        const problem = Field.emailProblem(value);

        if (problem !== null) {
            throw new Error(problem);
        }
    }

    static emailProblem(value: string): string | null {
        if (value.trim() === '') {
            return 'Digite o e-mail.';
        }

        return /^[^\s@]+@[^\s@]+$/.test(value.trim()) ? null : 'Esse e-mail não parece válido.';
    }

    static nicknameProblem(value: string): string | null {
        if (value.trim().length < 3 || value.trim().length > 32) {
            return 'O apelido precisa ter de 3 a 32 caracteres.';
        }

        return /^[A-Za-z0-9._]+$/.test(value.trim()) ? null : 'O apelido aceita letras, números, ponto e _ — sem espaço.';
    }

    static check(problems: Record<string, string | null>): void {
        const errors: Record<string, string[]> = {};

        for (const [field, problem] of Object.entries(problems)) {
            if (problem !== null) {
                errors[field] = [problem];
            }
        }

        const first = Object.values(errors)[0];

        if (first) {
            throw Object.assign(new Error(first[0]), { errors });
        }
    }

    static errors(failure: unknown, fields: string[]): Record<string, string> {
        const errors = (failure as { errors?: Record<string, string[] | undefined> } | null | undefined)?.errors ?? {};
        const found: Record<string, string> = {};

        for (const field of fields) {
            const message = errors[field]?.[0];

            if (message) {
                found[field] = message;
            }
        }

        return found;
    }
}
