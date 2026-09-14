const { invoke } = window.__TAURI__.core;

/**
 * Manda a tela para a sala, sempre pelo servidor.
 *
 * O upload sobe uma vez só, não importa quantos assistam: quatro pessoas em 1080p por
 * conexão direta custariam ~28 Mbps de subida, contra os mesmos ~7 Mbps daqui. Quem
 * chega depois é atendido sem novo aperto de mão, e o "ao vivo" acende sozinho, porque
 * publicar um producer já marca quem compartilha.
 *
 * O preço é a latência do salto — o servidor está nos EUA e as pessoas no Brasil, ~139 ms
 * em vez de ~20. Para quem assiste uma tela, isso não se percebe.
 */
export class Broadcast {
    constructor(sfu) {
        this.sfu = sfu;
        this.broadcasting = false;
        this.nativeActive = false;
        this.producerIds = [];

        /** O video que a sala recebe. E por ele que quem transmite consegue se ver. */
        this.videoProducerId = null;
    }

    async start(quality, fps, source, audio, muteCalls) {
        await invoke('start_broadcast', { quality, fps, source, audio, muteCalls });
        this.nativeActive = true;

        try {
            await this.publish();
        } catch (error) {
            await this.stop();
            throw error;
        }
    }

    /**
     * O servidor reiniciou e levou junto os producers e a porta de RTP. A captura aqui
     * nunca parou, então republicar é declarar de novo e reapontar o destino: a GPU não é
     * tocada e quem transmite não vê a transmissão piscar.
     *
     * Sem isto o app segue mandando pacote para uma porta que não existe mais, com o "ao
     * vivo" aceso, enquanto a sala inteira olha para tela preta. O `producerDead` também
     * não salva — aquele temporizador morreu junto com o processo do servidor.
     */
    async republish() {
        if (! this.nativeActive) {
            return false;
        }

        // Chave nova, e não a mesma: reconectar monta um contexto SRTP do zero, com
        // sequência aleatória nova. Repetir a chave com o contador reiniciado repetiria o
        // keystream, e dois trechos cifrados com o mesmo keystream se abrem um ao outro.
        await invoke('renew_sfu_key');

        this.producerIds = [];
        this.videoProducerId = null;

        await this.publish();

        return true;
    }

    /** Declara vídeo e áudio no servidor e aponta o RTP para a porta que ele devolveu. */
    async publish() {
        // Vídeo e áudio caem no mesmo transport do servidor, então o endereço é um só —
        // e é por isso que o `use_sfu` vem depois dos dois: mandar RTP antes de declarar
        // o áudio faria o servidor receber pacotes de um SSRC que ele ainda não conhece
        // e descartá-los em silêncio.
        let target = null;

        for (const source of ['screen', 'screenAudio']) {
            const kind = source === 'screen' ? 'video' : 'audio';
            const offer = await invoke('sfu_offer', { source });

            const producer = await this.sfu.request('producePlain', { kind, source, ...offer });

            this.producerIds.push(producer.producerId);

            if (source === 'screen') {
                this.videoProducerId = producer.producerId;
            }

            target = producer;
        }

        // A chave de SAÍDA do servidor vem na mesma resposta e sempre veio — o app
        // é que a jogava fora. É com ela que o Rust abre o caminho de volta e vê o
        // pedido de quadro-chave, que é o que encurta a travada de quem assiste.
        await invoke('use_sfu', {
            address: `${target.ip}:${target.port}`,
            serverKey: target.srtpParameters?.keyBase64 ?? null,
        });
        this.broadcasting = true;
    }

    /** Devolve quantos quadros foram transmitidos, para a mensagem de encerramento. */
    async stop() {
        if (! this.nativeActive && ! this.producerIds.length) {
            return 0;
        }

        this.broadcasting = false;

        const producerIds = this.producerIds.splice(0);

        this.videoProducerId = null;

        // Parar a captura não fecha os producers já registrados no mediasoup. Fechá-los
        // primeiro avisa todos os espectadores imediatamente, sem esperar o socket cair.
        await Promise.all(producerIds.map(producerId => this.sfu.tolerate('closeProducer', { producerId })));

        try {
            return await invoke('stop_broadcast');
        } finally {
            this.nativeActive = false;
        }
    }
}
