import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import importPlugin from 'eslint-plugin-import';
import prettier from 'eslint-plugin-prettier';
import prettierConfig from 'eslint-config-prettier';

export default [
    { ignores: ['dist'] },

    js.configs.recommended,

    ...tseslint.configs.recommended,

    {
        ignores: ['eslint.config.mjs'],
        files: ['**/*.ts'],

        languageOptions: {
            parser: tseslint.parser,
            parserOptions: {
                ecmaVersion: 2020,
                sourceType: 'module',
                tsconfigRootDir: import.meta.dirname,
            },
            globals: {
                ...globals.node,
                ...globals.browser,
                Atomics: 'readonly',
                SharedArrayBuffer: 'readonly',
            },
        },

        plugins: {
            '@typescript-eslint': tseslint.plugin,
            import: importPlugin,
            prettier: prettier,
        },

        settings: {
            'import/resolver': {
                typescript: {
                    alwaysTryTypes: true,
                },
                node: {
                    extensions: ['.js', '.jsx', '.ts', '.tsx'],
                },
            },
        },

        rules: {
            indent: ['error', 4],

            'no-multiple-empty-lines': ['error', { max: 1, maxEOF: 1, maxBOF: 0 }],

            quotes: ['error', 'single'],

            'import/newline-after-import': 'error',

            '@typescript-eslint/explicit-member-accessibility': [
                'error',
                { accessibility: 'explicit' },
            ],

            curly: ['error', 'all'],

            'import/order': [
                'error',
                {
                    pathGroups: [{ pattern: '@/**', group: 'internal' }],
                    pathGroupsExcludedImportTypes: ['builtin'],
                    groups: [
                        ['builtin', 'external'],
                        'internal',
                        ['parent', 'sibling', 'index'],
                    ],
                    'newlines-between': 'always',
                    alphabetize: {
                        order: 'asc',
                        caseInsensitive: true,
                    },
                },
            ],

            'prefer-const': 'error',
            '@typescript-eslint/no-unused-vars': [
                'error',
                {
                    argsIgnorePattern: '^_',
                    varsIgnorePattern: '^_',
                    caughtErrorsIgnorePattern: '^_',
                },
            ],

            eqeqeq: ['error', 'always'],

            '@typescript-eslint/no-explicit-any': 'error',

            'prettier/prettier': 'error',
        },
    },

    prettierConfig,
];
