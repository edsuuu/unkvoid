import { execFileSync, spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { SECRET } from './SfuProcess.mjs';

const WEB = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'web');
const CACHE = join(dirname(fileURLToPath(import.meta.url)), '..', '.cache');

/**
 * O Laravel de verdade (`web/`) num banco sqlite descartável, ligado ao SFU do cenário: é
 * quem assina o token de voz e quem move a pessoa de canal (`PATCH .../voice/members`).
 *
 * O storage e o cache de inicialização também são descartáveis (`e2e/.cache`): no `web/` de
 * quem roda, o log `sfu` do harness se misturava ao do dia, e ficavam views compiladas e o
 * `bootstrap/cache` de outro ambiente.
 *
 * `E2E_PHP` aponta o PHP 8.4 (o padrão é `php` do PATH). Sem PHP 8.4 ou sem o `vendor/`,
 * `unavailable()` diz por quê, e o cenário que depende dele é pulado com o motivo.
 */
export class Laravel {
    constructor({ port, sfu }) {
        this.port = port;
        this.sfu = sfu;
        this.php = process.env.E2E_PHP ?? 'php';
        this.url = `http://127.0.0.1:${port}`;
        this.database = join(CACHE, `laravel-${port}.sqlite`);
        this.storage = join(CACHE, `laravel-${port}-storage`);
        this.output = [];
        this.env = {
            APP_KEY: `base64:${randomBytes(32).toString('base64')}`,
            APP_ENV: 'local',
            APP_DEBUG: 'false',
            APP_URL: this.url,
            DB_CONNECTION: 'sqlite',
            DB_DATABASE: this.database,
            CACHE_STORE: 'database',
            SESSION_DRIVER: 'array',
            QUEUE_CONNECTION: 'sync',
            LOG_CHANNEL: 'stderr',
            BCRYPT_ROUNDS: '4',
            // Um worker só: com sqlite, dois escrevendo ao mesmo tempo (o webhook do SFU e o token)
            // dão "database is locked". Nada aqui espera o próprio Laravel, então não trava.
            PHP_CLI_SERVER_WORKERS: '1',
            SFU_URL: sfu.http,
            SFU_PUBLIC_URL: sfu.url,
            SFU_SECRET: SECRET,
            LARAVEL_STORAGE_PATH: this.storage,
            VIEW_COMPILED_PATH: join(this.storage, 'framework', 'views'),
            APP_PACKAGES_CACHE: join(this.storage, 'bootstrap', 'packages.php'),
            APP_SERVICES_CACHE: join(this.storage, 'bootstrap', 'services.php'),
            APP_CONFIG_CACHE: join(this.storage, 'bootstrap', 'config.php'),
            APP_ROUTES_CACHE: join(this.storage, 'bootstrap', 'routes.php'),
            APP_EVENTS_CACHE: join(this.storage, 'bootstrap', 'events.php'),
        };
    }

    static unavailable() {
        const php = process.env.E2E_PHP ?? 'php';

        if (!existsSync(join(WEB, 'vendor', 'autoload.php'))) {
            return 'web/vendor não existe (rode composer install em web/)';
        }

        try {
            const version = execFileSync(php, ['-r', 'echo PHP_VERSION;'], { encoding: 'utf8', timeout: 60_000 }).trim();
            const [major, minor] = version.split('.').map(Number);

            return major > 8 || (major === 8 && minor >= 4) ? null : `PHP ${version} (o web/ pede 8.4; aponte E2E_PHP)`;
        } catch (error) {
            return `PHP não encontrado (${php}): ${error.message.split('\n')[0]}`;
        }
    }

    async start() {
        mkdirSync(CACHE, { recursive: true });
        rmSync(this.database, { force: true });
        rmSync(this.storage, { recursive: true, force: true });
        writeFileSync(this.database, '');

        for (const folder of ['app', 'logs', 'bootstrap', join('framework', 'cache', 'data'), join('framework', 'sessions'), join('framework', 'views')]) {
            mkdirSync(join(this.storage, folder), { recursive: true });
        }

        const env = { ...process.env, ...this.env };

        execFileSync(this.php, ['artisan', 'migrate', '--force'], { cwd: WEB, env, stdio: 'pipe', timeout: 300_000 });

        this.process = spawn(this.php, ['-S', `127.0.0.1:${this.port}`, '../vendor/laravel/framework/src/Illuminate/Foundation/resources/server.php'], {
            cwd: join(WEB, 'public'),
            env,
            stdio: ['ignore', 'pipe', 'pipe'],
        });
        this.process.stdout.on('data', chunk => this.output.push(chunk.toString()));
        this.process.stderr.on('data', chunk => this.output.push(chunk.toString()));

        const deadline = Date.now() + 60_000;

        while (Date.now() < deadline) {
            const ok = await fetch(`${this.url}/api/config`, { signal: AbortSignal.timeout(2000) }).then(response => response.ok, () => false);

            if (ok) {
                return this;
            }

            await new Promise(resolve => setTimeout(resolve, 250));
        }

        throw new Error(`Laravel did not answer in 60 s:\n${this.output.join('').slice(-3000)}`);
    }

    async api(method, path, body = undefined, token = undefined) {
        const response = await fetch(`${this.url}/api${path}`, {
            method,
            headers: {
                accept: 'application/json',
                'content-type': 'application/json',
                ...(token ? { authorization: `Bearer ${token}` } : {}),
            },
            ...(body === undefined ? {} : { body: JSON.stringify(body) }),
            signal: AbortSignal.timeout(15_000),
        });
        const text = await response.text();
        let json = null;

        try {
            json = text ? JSON.parse(text) : null;
        } catch {
            json = { raw: text.slice(0, 500) };
        }

        return { status: response.status, body: json?.data ?? json };
    }

    async register(nickname) {
        const answer = await this.api('POST', '/auth/register', { email: `${nickname}@e2e.test`, password: 'senha-forte-123', device: 'e2e' });

        if (answer.status !== 200 && answer.status !== 201) {
            throw new Error(`register ${nickname}: ${answer.status} ${JSON.stringify(answer.body)}`);
        }

        return { token: answer.body.token, user: answer.body.user };
    }

    /** O token de voz que o app pede antes de cada `join` (`POST .../voice/token`). */
    voiceIdentity(account, channelId) {
        return async () => {
            const answer = await this.api('POST', `/channels/${channelId}/voice/token`, {}, account.token);

            if (answer.status !== 200) {
                throw Object.assign(new Error(`voice token ${answer.status}: ${JSON.stringify(answer.body)}\n${this.output.join('').slice(-2000)}`), { status: answer.status });
            }

            return { token: answer.body.token };
        };
    }

    async stop() {
        if (this.process && this.process.exitCode === null) {
            const exited = new Promise(resolve => this.process.once('exit', resolve));

            this.process.kill('SIGTERM');
            await Promise.race([exited, new Promise(resolve => setTimeout(resolve, 5000))]);

            if (this.process.exitCode === null) {
                this.process.kill('SIGKILL');
            }
        }

        rmSync(this.database, { force: true });
        rmSync(this.storage, { recursive: true, force: true });
    }
}
