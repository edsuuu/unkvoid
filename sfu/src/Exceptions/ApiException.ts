export class ApiException extends Error {
    constructor(message: string, public readonly status: number = 400) {
        super(message);
        this.name = new.target.name;
    }
}

export class ValidationException extends ApiException {
    constructor(message: string) {
        super(message, 422);
    }
}

export class UnauthorizedException extends ApiException {
    constructor(message = 'não autenticado') {
        super(message, 401);
    }
}

export class ForbiddenException extends ApiException {
    constructor(message = 'sem permissão para esta ação') {
        super(message, 403);
    }
}

export class NotFoundException extends ApiException {
    constructor(message = 'não encontrado') {
        super(message, 404);
    }
}
