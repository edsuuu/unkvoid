import { Request } from './Request.js';

const SOURCES = ['mic', 'camera', 'screen', 'screenAudio'];

export class ProduceRequest extends Request {
    validate() {
        this.string('transportId');
        this.enum('kind', ['audio', 'video']);
        this.enum('source', SOURCES);
        this.object('rtpParameters');
    }

    transportId() {
        return this.string('transportId');
    }

    kind() {
        return this.string('kind');
    }

    source() {
        return this.string('source');
    }

    rtpParameters() {
        return this.object('rtpParameters');
    }
}
