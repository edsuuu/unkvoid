export const Action = {
    Join: 'join',
    Leave: 'leave',
    RemovePeer: 'removePeer',
    Ping: 'ping',
    Identify: 'identify',
    Subscribe: 'subscribe',
    Unsubscribe: 'unsubscribe',
    CreateTransport: 'createTransport',
    ConnectTransport: 'connectTransport',
    Produce: 'produce',
    ProducePlain: 'producePlain',
    PauseProducer: 'pauseProducer',
    ResumeProducer: 'resumeProducer',
    CloseProducer: 'closeProducer',
    Consume: 'consume',
    ConsumePlain: 'consumePlain',
    ResumeConsumer: 'resumeConsumer',
    PauseConsumer: 'pauseConsumer',
    CloseConsumer: 'closeConsumer',
    VoiceState: 'voiceState',
    /** Pedido de entrar no grupo MLS, levando um KeyPackage opaco. */
    E2eeJoin: 'e2eeJoin',
    /** O único Commit que o SFU aceita para a época MLS atual. */
    E2eeCommit: 'e2eeCommit',
    /** Welcome opaco, entregue só depois do Commit correspondente ser aceito. */
    E2eeWelcome: 'e2eeWelcome',
} as const;

export type ActionName = (typeof Action)[keyof typeof Action];
