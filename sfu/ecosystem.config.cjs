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
                // O anel dos clipes. /var/tmp e não a pasta do deploy: o rsync --delete
                // apagaria a gravação de quem está no ar enquanto o install espera esvaziar.
                SFU_FFMPEG: 'ffmpeg',
                SFU_RECORDINGS_DIR: '/var/tmp',
                ...doArquivo,
            },
        },
        // O Reverb (chat e presença) mora aqui porque o pm2 desta máquina é um só, e um
        // processo fora do `pm2 save` é um processo que não volta depois do reboot.
        //
        // Ele NÃO é reiniciado pelo deploy do SFU (o install.sh passa `--only sfu`): parar
        // o chat porque a mídia subiu uma versão não tem motivo. Quem o reinicia é o
        // deploy do site, e **precisa** reiniciar: o processo abre o release que o
        // `current` apontava na hora em que subiu, e o deploy-web.sh apaga o release
        // antigo depois de três versões — sem o restart, o Reverb fica de pé segurando
        // uma pasta que já não existe.
        //
        // Primeira vez, na VPS:
        //   cd /var/www/projects/sfu && pm2 startOrRestart ecosystem.config.cjs --only reverb && pm2 save
        {
            name: 'reverb',
            script: 'artisan',
            args: 'reverb:start --host=127.0.0.1 --port=8080',
            interpreter: '/usr/bin/php8.4',
            cwd: '/var/www/projects/unkvoid-web/current',
            // UM processo, e não um por núcleo: dois Reverbs só compartilham quem está
            // escutando o quê através do Redis (`REVERB_SCALING_ENABLED`), que não existe
            // nesta máquina. Sem ele, a mensagem publicada no processo A não chega a
            // ninguém conectado no processo B — metade do chat desaparece em silêncio.
            instances: 1,
            exec_mode: 'fork',
            autorestart: true,
            // `watch` explícito porque o padrão do pm2 é vigiar o diretório: o Laravel
            // escreve em storage/logs a cada erro, e cada escrita viraria um restart que
            // derruba TODO WebSocket aberto. Presença é justamente o que não sobrevive a
            // um restart em loop.
            watch: false,
            // O processo de hoje ocupa 61 MB. 200M é teto de vazamento, não de operação.
            max_memory_restart: '200M',
        },
    ],
};
