export const Action = {
    Join: 'join',
    Leave: 'leave',
    RemovePeer: 'removePeer',
    CreateTransport: 'createTransport',
    ConnectTransport: 'connectTransport',
    ProducePlain: 'producePlain',
    CloseProducer: 'closeProducer',
    Consume: 'consume',
    ResumeConsumer: 'resumeConsumer',
    PauseConsumer: 'pauseConsumer',
} as const;

export type ActionName = (typeof Action)[keyof typeof Action];
