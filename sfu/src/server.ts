import { App } from './app.js';
import { config } from './Config/index.js';

// O mediasoup escuta SIGINT e SIGTERM para fechar os workers, e um listener basta para o
// Node não sair mais sozinho no sinal: o pm2 pedia para parar, os workers fechavam e o
// processo ficava de pé, com o /health dizendo ok e todo join morrendo em
// "Channel closed". Sair aqui devolve o sinal ao que ele era.
for (const signal of ['SIGINT', 'SIGTERM'] as const) {
    process.once(signal, () => {
        console.log(`[INFO] ${signal} received, closing the SFU`);
        process.exit(0);
    });
}

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
