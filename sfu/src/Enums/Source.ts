export const Source = {
    Screen: 'screen',
    ScreenAudio: 'screenAudio',
    Mic: 'mic',
    Camera: 'camera',
} as const;

export type SourceName = (typeof Source)[keyof typeof Source];

export const SOURCES: readonly SourceName[] = Object.values(Source);

/** O que o token precisa carregar em `can` para cada origem. */
export const PERMISSION_BY_SOURCE: Record<SourceName, string> = {
    screen: 'stream',
    screenAudio: 'stream',
    mic: 'speak',
    camera: 'video',
};
