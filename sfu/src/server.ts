import { App } from './app.js';
import { config } from './Config/index.js';

new App()
    .boot()
    .then((http) =>
        http.listen(config.listenPort, config.listenHost, () =>
            console.log(
                `[INFO] SFU em ${config.listenHost}:${config.listenPort}${config.path} · media on port ${config.mediaPort}`,
            ),
        ),
    )
    .catch((exception: unknown) => {
        console.error('[ERROR] failed to start the SFU', exception);
        process.exit(1);
    });
