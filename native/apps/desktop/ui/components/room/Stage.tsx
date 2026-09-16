import type { ReactNode } from 'react';

import { Icon } from '../common/Icon.tsx';
import { Spinner } from '../common/Spinner.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { StreamTile } from './StreamTile.tsx';

type StageProps = {
    canShare: boolean;
    hint?: ReactNode;
    compact?: boolean;
};

export function Stage({ canShare, hint = null, compact = false }: StageProps) {
    const app = useApp();
    const media = app.media;
    const { tiles, focused, fullscreen, pending, connecting, reconnecting, paused, audio, nativeMuted, image, idle, watchers } = useStore(media.store);
    const { active } = useStore(app.sharing.store);
    const focusedTile = tiles.find(tile => tile.key === focused) ?? null;
    const others = tiles.filter(tile => tile !== focusedTile);
    const focusing = Boolean(focusedTile) && others.length > 0 && ! fullscreen;
    let columns = `repeat(${tiles.length <= 1 ? 1 : tiles.length <= 4 ? 2 : 3}, minmax(0, 1fr))`;

    if (compact) {
        columns = 'repeat(auto-fit, minmax(min(100%, 180px), 1fr))';
    }

    if (focusing) {
        columns = `repeat(${Math.max(2, Math.min(4, others.length))}, minmax(0, 1fr))`;
    }

    if (connecting) {
        return (
            <div className="flex flex-1 animate-fade-in items-center justify-center">
                <div className="glass flex flex-col items-center gap-4 rounded-3xl px-10 py-9">
                    <Spinner size={26} />
                    <p className="text-[14px] font-medium">Entrando na sala…</p>
                    <p className="text-[12px] text-ink-dim">Conectando ao servidor de mídia</p>
                </div>
            </div>
        );
    }

    if (tiles.length === 0) {
        return (
            <div className="flex flex-1 animate-fade-in items-center justify-center p-4">
                <div className="glass w-full max-w-[520px] rounded-3xl p-9 text-center shadow-[0_30px_80px_-40px_rgba(0,0,0,0.95)]">
                    <div className="relative mx-auto mb-5 flex size-16 items-center justify-center overflow-hidden rounded-[20px] border border-brand/30 bg-brand/15 text-lilac-2">
                        <span className="absolute inset-0 animate-sweep bg-gradient-to-r from-transparent via-brand/25 to-transparent" />
                        <Icon name="screen" size={26} />
                    </div>
                    <h5 className="m-0 text-[19px] font-semibold tracking-tight">
                        {pending ? 'Alguém está compartilhando, mas a tela não abriu sozinha.' : active ? 'Você está transmitindo.' : 'Ninguém está compartilhando ainda.'}
                    </h5>
                    {active && ! pending && <p className="mt-2.5 text-[13px] text-ink-soft">A sua tela não aparece aqui para não gastar um decoder à toa.</p>}
                    {hint && ! active && <p className="mt-2.5 text-[13px] text-ink-soft">{hint}</p>}
                    <div className="mt-5 flex flex-wrap justify-center gap-2">
                        {pending && <button className="btn-ghost px-4 py-2.5 text-[13px]" type="button" onClick={() => void media.refreshWatch()}>Assistir</button>}
                        {active && <button className="btn-ghost px-4 py-2.5 text-[13px]" type="button" onClick={() => void media.toggleSelfView()}>Ver o que a sala vê</button>}
                        {canShare && ! active && <button className="btn-primary px-5 text-[13px]" type="button" onClick={() => void app.sharing.open()}>Iniciar compartilhamento</button>}
                    </div>
                    {reconnecting && <p className="mt-4 font-mono text-[11px] text-danger">reconectando…</p>}
                </div>
            </div>
        );
    }

    return (
        <div className="flex min-h-0 flex-1 animate-fade-in flex-col gap-2.5">
            <div className="flex flex-wrap items-center gap-2">
                <span className="label-mono">Transmissões</span>
                <span className="rounded-full border border-brand/30 bg-brand/15 px-2 py-0.5 font-mono text-[10px] text-lilac-2">
                    {tiles.length === 1 ? '1 tela' : `${tiles.length} telas`}
                </span>
                {reconnecting && <span className="flex items-center gap-1.5 font-mono text-[10.5px] text-danger"><Spinner size={10} />reconectando…</span>}
                <span className="flex-1" />
                {pending && <button className="btn-ghost px-2.5 py-1.5 text-[11.5px]" type="button" onClick={() => void media.refreshWatch()}>Assistir quem falta</button>}
                {focusedTile && <button className="btn-ghost px-2.5 py-1.5 text-[11.5px]" type="button" onClick={() => media.focus(focusedTile.key)}>Sair do foco</button>}
                <button className="btn-icon size-7 rounded-[9px]" type="button" title="Tela cheia" onClick={() => void media.toggleFullscreen(focusedTile?.key ?? tiles[0].key)}>
                    <Icon name="fullscreen" size={13} />
                </button>
            </div>

            <div
                className="grid min-h-0 flex-1 gap-2.5"
                style={{
                    gridTemplateColumns: columns,
                    gridTemplateRows: focusing ? 'minmax(0, 1fr) 6.5rem' : undefined,
                    gridAutoRows: focusing ? undefined : 'minmax(0, 1fr)',
                }}
            >
                {tiles.map(tile => (
                    <StreamTile
                        key={tile.key}
                        tile={tile}
                        focused={tile === focusedTile}
                        thumb={focusing && tile !== focusedTile}
                        full={fullscreen === tile.key}
                        hidden={Boolean(fullscreen) && fullscreen !== tile.key}
                        paused={paused.includes(tile.key)}
                        volume={audio[tile.key]?.volume ?? null}
                        muted={audio[tile.key]?.muted ?? null}
                        nativeMuted={nativeMuted[tile.key] ?? true}
                        watchers={watchers[tile.key] ?? []}
                        brightness={image.brightness}
                        contrast={image.contrast}
                        saturation={image.saturation}
                        idle={idle}
                    />
                ))}
            </div>
        </div>
    );
}
