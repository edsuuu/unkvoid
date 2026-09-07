export const Action = {
    Join: 'join',
    Leave: 'leave',
    Signal: 'signal',
    WatchServer: 'watchServer',
    CreateTransport: 'createTransport',
    ConnectTransport: 'connectTransport',
    Produce: 'produce',
    ProducePlain: 'producePlain',
    CloseProducer: 'closeProducer',
    Consume: 'consume',
    ResumeConsumer: 'resumeConsumer',
    PauseConsumer: 'pauseConsumer',
    SetPreferredLayers: 'setPreferredLayers',
    StopBroadcast: 'stopBroadcast',
    DisconnectPeer: 'disconnectPeer',
} as const;

export type ActionName = (typeof Action)[keyof typeof Action];
