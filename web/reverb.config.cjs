/**
 * Reverb sob pm2, junto do SFU.
 *
 * A porta sai do .env, não de uma constante aqui: o nginx e o Laravel já leem de lá, e
 * uma quarta cópia do número é a que ninguém lembra de mudar. Foi assim que a primeira
 * tentativa subiu na 8080 e colidiu com o filebrowser que já mora nesta VPS.
 *
 * Uma instância só: o Reverb guarda as conexões em memória, então duas teriam cada uma
 * metade das pessoas e uma mensagem chegaria para metade da sala. Escalar exige o driver
 * de escala do Reverb (Redis), que esta VPS não tem.
 */
const fs = require('node:fs');
const path = require('node:path');

const env = (key, fallback) => {
    try {
        const arquivo = fs.readFileSync(path.join(__dirname, '.env'), 'utf8');
        const achado = arquivo.match(new RegExp(`^${key}=(.*)$`, 'm'));

        return achado ? achado[1].trim().replace(/^["']|["']$/g, '') : fallback;
    } catch {
        return fallback;
    }
};

module.exports = {
    apps: [
        {
            name: 'reverb',
            cwd: __dirname,
            script: 'artisan',
            interpreter: 'php',
            args: `reverb:start --host=${env('REVERB_HOST', '127.0.0.1')} --port=${env('REVERB_PORT', '8081')}`,
            instances: 1,
            exec_mode: 'fork',
            autorestart: true,
            max_memory_restart: '256M',
        },
    ],
};
