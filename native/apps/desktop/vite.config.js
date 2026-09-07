import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';

// The desktop UI is the same product as the web one, so it uses the same voice code
// instead of a copy: SfuClient, MicrophoneGate and PresenceClient live in web/ and are
// imported from here. Two copies of a reconnect protocol drift, and the one that drifts
// is always the one nobody is looking at.
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
        },
    },
    server: { port: 1420, strictPort: true },
    clearScreen: false,
});
