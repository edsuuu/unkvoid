export const PERMISSION_BY_SOURCE = {
    screen: 'stream',
    screenAudio: 'stream',
    mic: 'speak',
    camera: 'video',
} as const;

export type SourceName = keyof typeof PERMISSION_BY_SOURCE;

export const SOURCES = Object.keys(PERMISSION_BY_SOURCE) as readonly SourceName[];
