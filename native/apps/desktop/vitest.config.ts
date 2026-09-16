import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

export default defineConfig({
    test: {
        projects: [
            {
                plugins: [react()],
                test: {
                    name: 'unit',
                    include: ['tests/unit/**/*.test.{ts,tsx}'],
                    environment: 'jsdom',
                    environmentOptions: { jsdom: { url: 'http://localhost/', pretendToBeVisual: true } },
                },
            },
            {
                test: {
                    name: 'integration',
                    include: ['tests/integration/**/*.test.ts'],
                    environment: 'node',
                    testTimeout: 60_000,
                    hookTimeout: 30_000,
                },
            },
        ],
    },
});
