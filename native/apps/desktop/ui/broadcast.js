const { invoke } = window.__TAURI__.core;

/**
 * Manda a tela para a sala, sempre pelo servidor.
 *
 * Antes havia um caminho direto entre pares, para até três espectadores, que só existia
 * no código do app. Quem assistia pela web nunca recebia nada — não é que ficasse ruim,
 * é que não chegava. E o caminho direto ainda multiplicava o upload de quem transmite:
 * quatro pessoas em 1080p custavam ~28 Mbps de subida, contra os mesmos ~7 Mbps do
 * servidor para qualquer plateia.
 *
 * Pelo servidor todo mundo consome igual — web e app —, quem chega depois é atendido
 * sem novo aperto de mão, e o "ao vivo" acende sozinho, porque publicar um producer já
 * marca quem compartilha. O preço é a latência do salto: o servidor está nos EUA e as
 * pessoas no Brasil, o que custa ~139 ms em vez de ~20. Para quem assiste uma tela,
 * isso não se percebe.
 */
export class Broadcast {
    constructor(sfu) {
        this.sfu = sfu;
        this.broadcasting = false;
    }

    async start(quality, source) {
        // `iceServers` é resto do caminho direto: o Rust ainda pede o argumento, e o
        // que ele monta com ele — `offer_to`, `accept_answer`, o `PeerLink` inteiro —
        // não é mais chamado por ninguém. Sai numa limpeza própria, que é só deleção e
        // merece ser verificada sozinha.
        await invoke('start_broadcast', { quality, source, iceServers: [] });

        // Vídeo e áudio caem no mesmo transport do servidor, então o endereço é um só —
        // e é por isso que o `use_sfu` vem depois dos dois: mandar RTP antes de declarar
        // o áudio faria o servidor receber pacotes de um SSRC que ele ainda não conhece
        // e descartá-los em silêncio.
        let target = null;

        for (const kind of ['video', 'audio']) {
            const offer = await invoke('sfu_offer', { kind });

            target = await this.sfu.request('producePlain', {
                kind,
                source: kind === 'video' ? 'screen' : 'screenAudio',
                ...offer,
            });
        }

        await invoke('use_sfu', { address: `${target.ip}:${target.port}` });

        this.broadcasting = true;
    }

    /** Devolve quantos quadros foram transmitidos, para a mensagem de encerramento. */
    async stop() {
        if (! this.broadcasting) {
            return 0;
        }

        this.broadcasting = false;

        return invoke('stop_broadcast');
    }
}
