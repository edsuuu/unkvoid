export const Source = {
    Microphone: 'mic',
    Camera: 'camera',
    Screen: 'screen',
    ScreenAudio: 'screenAudio',
} as const;

export type SourceName = (typeof Source)[keyof typeof Source];

export const SOURCES: readonly SourceName[] = Object.values(Source);
