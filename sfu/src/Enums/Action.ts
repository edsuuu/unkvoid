export const Action = {
    Join: 'join',
    Leave: 'leave',
    RemovePeer: 'removePeer',
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
} as const;

export type ActionName = (typeof Action)[keyof typeof Action];
