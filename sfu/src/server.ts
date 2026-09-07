import { Server } from './Http/Server.js';

new Server().start().catch((exception: unknown) => {
    console.error('[ERRO] falha ao subir o SFU', exception);
    process.exit(1);
});
