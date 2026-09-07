import Echo from 'laravel-echo';
import Pusher from 'pusher-js';

window.Pusher = Pusher;

/**
 * The connection settings come from the page, not from the bundle.
 *
 * The assets are built on a developer machine and shipped ready, so a build-time
 * variable would carry "localhost" into production. Read at runtime, the same bundle
 * works in both places — and there is no way to ship one pointing at the wrong server.
 */
const config = name => document.querySelector(`meta[name="reverb-${name}"]`)?.content ?? '';

const key = config('key');

if (key) {
    const scheme = config('scheme') || 'https';
    const port = Number(config('port')) || (scheme === 'https' ? 443 : 80);

    window.Echo = new Echo({
        broadcaster: 'reverb',
        key,
        wsHost: config('host') || window.location.hostname,
        wsPort: port,
        wssPort: port,
        forceTLS: scheme === 'https',
        enabledTransports: ['ws', 'wss'],
    });
}
