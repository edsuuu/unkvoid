import { createRequire } from 'node:module';

import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

const require = createRequire(import.meta.url);

export default defineConfig({
    plugins: [tailwindcss()],
    root: 'ui',
    build: {
        outDir: '../dist',
        emptyOutDir: true,
        target: 'chrome110',
    },
    resolve: {
        alias: { 'mediasoup-client': require.resolve('mediasoup-client') },
    },
    server: { port: 1420, strictPort: true },
    clearScreen: false,
});
