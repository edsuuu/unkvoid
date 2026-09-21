const { existsSync, readFileSync } = require('node:fs');
const { join } = require('node:path');

// O `.env` ao lado (fora do repositório) vence o que está aqui: é assim que esta máquina
// tem endereço e portas próprios sem editar um arquivo versionado. Veja o `.env.example`.
// O `.env` de produção morava em /var/www/projects/sfu quando o deploy era por rsync.
// O segundo caminho existe só para a migração: some quando ele for movido para cá.
const envPath = [join(__dirname, '.env'), '/var/www/projects/sfu/.env'].find(existsSync);
const doArquivo = envPath
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
            cwd: __dirname,
            instances: 1,
            autorestart: true,
            // Data e hora em cada linha do log: é o que diz quando alguém entrou, de que IP,
            // e em que ordem as coisas quebraram.
            time: true,
            max_memory_restart: '600M',
            env: {
                NODE_ENV: 'production',
                SFU_HOST: '127.0.0.1',
                SFU_PORT: '3000',
                SFU_APP_VERSION: '0.0.3',
                // O IP público da máquina NÃO fica aqui: ele muda de VPS para VPS, e um IP
                // versionado é um IP que continua apontando para a máquina desligada. Vem do
                // `.env` ao lado, e sem ele o SFU anuncia 127.0.0.1 e ninguém vê nada — o
                // item 11 da checagem do infra/INSTALAR-VPS.md existe para pegar isso.
                SFU_MEDIA_PORT: '40000',
                // Um worker por core MENOS UM: a VPS tem 8, e o oitavo fica para o nginx, o
                // php-fpm, o MySQL, o MinIO e o e-mail. A sala é fixada num worker e a voz é
                // presa a um núcleo, então 7 workers são 7 canais de voz pesados em paralelo.
                // Cada um usa uma porta a partir de SFU_MEDIA_PORT: 40000-40006, todas
                // liberadas no firewall.
                SFU_WORKERS: '7',
                // Teto de conexões novas por IP por minuto. Uma escola inteira atrás de
                // um NAT só precisa caber aqui.
                SFU_CONNECTIONS_PER_MINUTE: '120',
                // RTP puro do app: 64 portas por worker, 7 × 64 = 448, ou seja
                // 41000-41447 — dentro da regra 41000-42000 do firewall. Quem participa
                // da voz pelo Linux gasta duas (envia e recebe). A faixa PRECISA estar
                // aberta no firewall, senão o app conecta, publica e ninguém vê nada —
                // os pacotes morrem antes de chegar.
                SFU_PLAIN_PORT: '41000',
                SFU_PLAIN_PORTS: '64',
                // Para onde vai o aviso de quem entrou e saiu de um canal.
                SFU_LARAVEL_URL: 'https://unkvoid.com',
                ...doArquivo,
            },
        },
    ],
};
