// Não há mais segredo para guardar fora do repositório: a sala é anônima e o SFU não
// verifica assinatura nenhuma. Tudo o que ele precisa está aqui.
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
                // Um worker por core (a VPS tem 4). Cada um usa uma porta a partir de
                // SFU_MEDIA_PORT: 40000-40003, todas liberadas no firewall.
                SFU_WORKERS: '4',
            },
        },
    ],
};
