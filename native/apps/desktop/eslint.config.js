import js from '@eslint/js';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';
import { defineConfig } from 'eslint/config';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default defineConfig(
    {
        files: ['ui/**/*.{ts,tsx}', 'tests/**/*.{ts,tsx}'],
        extends: [js.configs.recommended, tseslint.configs.recommended],
        languageOptions: {
            globals: { ...globals.browser },
        },
        rules: {
            curly: ['error', 'all'],
            eqeqeq: ['error', 'always', { null: 'ignore' }],
            'prefer-const': 'error',
            quotes: ['error', 'single', { avoidEscape: true }],
            '@typescript-eslint/no-explicit-any': 'error',
        },
    },
    {
        files: ['ui/**/*.tsx'],
        extends: [react.configs.flat.recommended, react.configs.flat['jsx-runtime'], reactHooks.configs.flat.recommended],
        settings: {
            react: { version: 'detect' },
        },
    },
    {
        files: ['tests/**/*.{ts,tsx}'],
        languageOptions: {
            globals: { ...globals.node },
        },
    },
);
