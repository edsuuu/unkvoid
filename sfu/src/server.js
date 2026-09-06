import { Server } from './Http/Server.js';

new Server().start().catch(exception => {
    console.error('[ERRO] falha ao subir o SFU', exception);
    process.exit(1);
});
