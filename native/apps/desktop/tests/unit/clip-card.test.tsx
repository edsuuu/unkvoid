import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { AppContext } from '../../ui/components/AppContext.ts';
import { ClipCard } from '../../ui/components/clips/ClipCard.tsx';
import type { App } from '../../ui/core/App.ts';
import type { Clip } from '../../ui/core/Models.ts';

const DAY = 86_400_000;

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const clip = (status: Clip['status'], extra: Partial<Clip> = {}): Clip => ({
    id: status,
    status,
    streamer: { id: 40, name: 'Fulano' },
    server_name: 'Meu servidor',
    channel_name: 'Geral',
    duration_ms: 187_000,
    size_bytes: 1000,
    created_at: new Date().toISOString(),
    expires_at: new Date(Date.now() + 7 * DAY).toISOString(),
    thumbnail_url: null,
    playlist_url: null,
    download_url: null,
    ...extra,
});

describe('cartão de clipe: o que cada status mostra', () => {
    const app = { hub: { clips: { play() {}, download: async () => {}, remove: async () => {} } } } as unknown as App;
    let container: HTMLDivElement;
    let root: Root;

    const render = (value: Clip) => act(() => root.render(
        <AppContext.Provider value={app}>
            <ClipCard clip={value} />
        </AppContext.Provider>,
    ));
    const buttons = () => [...container.querySelectorAll('button')].map(button => button.textContent?.trim());

    beforeEach(() => {
        container = document.createElement('div');
        document.body.appendChild(container);
        root = createRoot(container);
    });

    afterEach(() => {
        act(() => root.unmount());
        container.remove();
    });

    it('pronto, com playlist e download: miniatura, Assistir, Baixar, e a duração com a validade', () => {
        render(clip('ready', {
            thumbnail_url: 'http://minio/thumb.jpg',
            playlist_url: 'http://api/clips/ready/playlist.m3u8?signature=a',
            download_url: 'http://minio/clips/ready/clip.mp4?X-Amz-Signature=c',
        }));

        expect(container.querySelector('img')?.getAttribute('src')).toBe('http://minio/thumb.jpg');
        expect(buttons()).toContain('Assistir');
        expect(buttons()).toContain('Baixar');
        expect(container.textContent).toMatch(/3:07 · some em 7 dias/);
    });

    it('processando, sem playlist nem download: "Salvando o clipe…", sem Assistir e sem Baixar', () => {
        render(clip('processing', { duration_ms: null }));

        expect(container.textContent).toContain('Salvando o clipe…');
        expect(buttons(), 'sem playlist, sem Assistir').not.toContain('Assistir');
        expect(buttons(), 'sem download_url, sem Baixar').not.toContain('Baixar');
    });

    it('falhou: diz que não deu para salvar, e não oferece Assistir', () => {
        render(clip('failed'));

        expect(container.textContent).toContain('Não deu para salvar este clipe.');
        expect(buttons()).not.toContain('Assistir');
    });
});
