const { existsSync, readFileSync } = require('node:fs');
const { join } = require('node:path');

// O `.env` ao lado (fora do repositório) vence o que está aqui: é assim que esta máquina
// tem endereço e portas próprios sem editar um arquivo versionado. Veja o `.env.example`.
const envPath = join(__dirname, '.env');
const doArquivo = existsSync(envPath)
    ? Object.fromEntries(
        readFileSync(envPath, 'utf8')
            .split('\n')
            .filter(linha => linha.trim() && ! linha.startsWith('#'))
            .map(linha => {
                const igual = linha.indexOf('=');

                return [linha.slice(0, igual).trim(), linha.slice(igual + 1).trim()];
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
                SFU_APP_VERSION: '0.0.3',
                SFU_ANNOUNCED_ADDRESS: '144.126.133.10',
                SFU_MEDIA_PORT: '40000',
                // Um worker por core (a VPS tem 4). Cada um usa uma porta a partir de
                // SFU_MEDIA_PORT: 40000-40003, todas liberadas no firewall.
                SFU_WORKERS: '4',
                // RTP puro de quem transmite pelo app: 8 portas por worker, 41000-41031.
                // A faixa PRECISA estar aberta no firewall, senão o app conecta, publica
                // e ninguém vê nada — os pacotes morrem antes de chegar.
                SFU_PLAIN_PORT: '41000',
                SFU_PLAIN_PORTS: '8',
                ...doArquivo,
            },
        },
    ],
};
