const { existsSync, readFileSync } = require('node:fs');
const { join } = require('node:path');

// Segredos moram no .env ao lado (fora do repo), não neste arquivo versionado.
const envPath = join(__dirname, '.env');
const fromFile = existsSync(envPath)
    ? Object.fromEntries(
        readFileSync(envPath, 'utf8')
            .split('\n')
            .filter(line => line.trim() && ! line.startsWith('#'))
            .map(line => {
                const index = line.indexOf('=');

                return [line.slice(0, index), line.slice(index + 1)];
            }),
    )
    : {};

module.exports = {
    apps: [
        {
            name: 'sfu',
            script: 'dist/server.js',
            cwd: '/var/www/projects/sfu',
            instances: 1,
            autorestart: true,
            max_memory_restart: '600M',
            env: {
                NODE_ENV: 'production',
                SFU_HOST: '127.0.0.1',
                SFU_PORT: '3000',
                SFU_ANNOUNCED_ADDRESS: '144.126.133.10',
                SFU_MEDIA_PORT: '40000',
                ...fromFile,
            },
        },
    ],
};
