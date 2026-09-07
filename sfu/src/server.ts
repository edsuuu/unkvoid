import { Server } from './Http/Server.js';

new Server().start().catch((exception: unknown) => {
    console.error('[ERROR] failed to start the SFU', exception);
    process.exit(1);
});
