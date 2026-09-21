export class ApiException extends Error {
    public constructor(
        message: string,
        public readonly status: number = 400,
    ) {
        super(message);
        this.name = new.target.name;
    }
}

export class ValidationException extends ApiException {
    public constructor(message: string) {
        super(message, 422);
    }
}

export class UnauthorizedException extends ApiException {
    public constructor(message = 'not authenticated') {
        super(message, 401);
    }
}

export class ForbiddenException extends ApiException {
    public constructor(message = 'not authorized for this action') {
        super(message, 403);
    }
}

export class NotFoundException extends ApiException {
    public constructor(message = 'not found') {
        super(message, 404);
    }
}

export class ServiceUnavailableException extends ApiException {
    public constructor(message = 'service unavailable') {
        super(message, 503);
    }
}
