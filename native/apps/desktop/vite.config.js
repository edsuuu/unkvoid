import { createRequire } from 'node:module';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const require = createRequire(import.meta.url);

export default defineConfig({
    plugins: [react(), tailwindcss()],
    root: 'ui',
    build: {
        outDir: '../dist',
        emptyOutDir: true,
        target: 'chrome110',
    },
    resolve: {
        alias: { 'mediasoup-client': require.resolve('mediasoup-client') },
    },
    server: {
        port: 1420,
        strictPort: true,
        // Só no `npm run dev` aberto no navegador: um host só, como o nginx da VPS — /api e
        // /broadcasting no Laravel, /health e /sfu no SFU. Sem isto o navegador recusa por CORS.
        proxy: {
            '/api': 'http://127.0.0.1:8000',
            '/broadcasting': 'http://127.0.0.1:8000',
            '/health': 'http://127.0.0.1:3000',
            '/sfu': { target: 'ws://127.0.0.1:3000', ws: true },
        },
    },
    clearScreen: false,
});
