export class StatusResource {
    constructor(status = 'ok') {
        this.status = status;
    }

    toArray() {
        return { status: this.status };
    }
}
