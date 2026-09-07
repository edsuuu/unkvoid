import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'vite';

const require = createRequire(import.meta.url);

// The desktop UI is the same product as the web one, so it uses the same voice code
// instead of a copy: SfuClient, MicrophoneGate and PresenceClient live in web/ and are
// imported from here. Two copies of a reconnect protocol drift, and the one that drifts
// is always the one nobody is looking at.
//
// Those files sit outside this package, so Node resolution walks up from web/ and never
// reaches this node_modules — every npm import they make has to be pinned here by hand.
// It worked locally only because web/node_modules happened to be installed next to them.
export default defineConfig({
    root: 'ui',
    build: {
        outDir: '../dist',
        emptyOutDir: true,
        target: 'chrome110',
    },
    resolve: {
        alias: {
            '@voice': fileURLToPath(new URL('../../../web/resources/js/voice', import.meta.url)),
            'mediasoup-client': require.resolve('mediasoup-client'),
        },
    },
    server: { port: 1420, strictPort: true },
    clearScreen: false,
});
