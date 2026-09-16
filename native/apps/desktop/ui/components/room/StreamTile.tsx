import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import { Failure } from '../../core/Failure.ts';
import type { Tile } from '../../core/Media.ts';
import { Avatar } from '../common/Avatar.tsx';
import { Icon } from '../common/Icon.tsx';
import { Popover } from '../common/Popover.tsx';
import { Spinner } from '../common/Spinner.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

const TILE_BUTTON = 'flex size-6 flex-none cursor-pointer items-center justify-center rounded-[7px] border border-line-strong bg-row text-ink-icon transition hover:border-brand/50 hover:text-ink-strong';

type StreamTileProps = {
    tile: Tile;
    focused: boolean;
    thumb: boolean;
    full: boolean;
    hidden: boolean;
    paused: boolean;
    volume: number | null;
    muted: boolean | null;
    nativeMuted: boolean;
    brightness: number;
    contrast: number;
    saturation: number;
    idle: boolean;
};

export const StreamTile = memo(function StreamTile({ tile, focused, thumb, full, hidden, paused, volume, muted, nativeMuted, brightness, contrast, saturation, idle }: StreamTileProps) {
    const media = useApp().media;
    const stats = useStore(media.stats)[tile.key];
    const video = useRef<HTMLVideoElement | null>(null);
    const [panelOpen, setPanelOpen] = useState(false);
    const [ready, setReady] = useState(false);
    const camera = tile.kind === 'camera';
    const native = tile.native;
    const tuned = brightness !== 100 || contrast !== 100 || saturation !== 100;
    const filter = paused
        ? `blur(8px) grayscale(1) brightness(${0.6 * brightness / 100})`
        : tuned ? `brightness(${brightness / 100}) contrast(${contrast / 100}) saturate(${saturation / 100})` : undefined;

    const attachVideo = useCallback((element: HTMLVideoElement | null) => {
        if (video.current && video.current !== element) {
            media.unregisterVideo(tile.key, video.current);
        }

        video.current = element;

        if (! element || ! tile.stream) {
            return;
        }

        element.srcObject = tile.stream;
        media.registerVideo(tile.key, element);
    }, [tile.stream, tile.key, media]);

    useEffect(() => {
        const element = video.current;

        if (! element || ! tile.stream) {
            return;
        }

        if (paused) {
            element.pause();

            return;
        }

        void element.play().catch((failure: unknown) => media.app.log('media.video.play.error', { peerId: tile.key, message: Failure.message(failure) }));
    }, [paused, full, tile.stream, tile.key, media]);

    const liveStats = stats && ! stats.paused ? stats : null;
    const lossHigh = (liveStats?.loss ?? 0) >= 2;
    const statsTitle = liveStats
        ? `${liveStats.ping ?? '--'} ms · ${liveStats.rate === null ? '--' : liveStats.rate.toFixed(1)} Mb/s · buffer ${liveStats.buffer.toFixed(1)} s · ${liveStats.totalLost ?? '--'} pacotes perdidos no total · jitter ${liveStats.jitter ?? '--'} ms`
        : '';
    const imageControls = [['brightness', 'Brilho', brightness], ['contrast', 'Contraste', contrast], ['saturation', 'Saturação', saturation]] as const;

    const figure = (
        <figure
            className={full ? `fixed inset-0 z-[60] m-0 flex flex-col bg-black ${idle ? 'cursor-none' : ''}` : 'relative m-0 flex min-h-0 min-w-0 flex-col gap-2'}
            hidden={hidden}
            style={focused ? { gridColumn: '1 / -1' } : undefined}
        >
            <div
                className={`relative min-h-0 flex-1 overflow-hidden bg-black ${full ? '' : 'rounded-[14px]'} ${thumb ? 'cursor-pointer ring-1 ring-line-strong hover:ring-brand/60' : ''}`}
                title={thumb ? 'Focar esta tela' : undefined}
                role={thumb ? 'button' : undefined}
                tabIndex={thumb ? 0 : undefined}
                onKeyDown={thumb ? event => event.key === 'Enter' && media.focus(tile.key) : undefined}
                onClick={thumb ? () => media.focus(tile.key) : undefined}
                onDoubleClick={thumb || camera ? undefined : () => void media.toggleFullscreen(tile.key)}
            >
                {native
                    ? <img className="size-full object-contain" src={`http://127.0.0.1:${native.port}/`} alt="" style={{ filter }} onError={() => { media.app.log('media.native.image.error', { producerId: native.producerId, port: native.port }); media.app.fail('a imagem desta transmissão parou de chegar: feche o cartão e use Assistir.'); }} />
                    : <video ref={attachVideo} className="size-full object-contain" autoPlay playsInline muted style={{ filter }} onLoadedData={() => setReady(true)} />}

                {! native && ! ready && ! paused && (
                    <span className="absolute inset-0 flex items-center justify-center overflow-hidden bg-gradient-to-br from-brand-dark/25 to-black text-lilac-2">
                        <span className="absolute inset-0 animate-sweep bg-gradient-to-r from-transparent via-brand/20 to-transparent" />
                        <Spinner size={20} />
                    </span>
                )}

                {paused && ! thumb && (
                    <button className="absolute inset-0 flex cursor-pointer items-center justify-center text-white/90 transition hover:text-white" type="button" aria-label="Retomar esta transmissão" onClick={() => void media.togglePause(tile.key)}>
                        <Icon name="play" size={64} />
                    </button>
                )}

                <span className={`live-badge absolute top-2 left-2 transition-opacity duration-200 ${camera ? 'bg-brand-dark' : ''} ${full && idle ? 'opacity-0' : ''}`}>
                    <span className="size-1 rounded-full bg-white" />
                    {camera ? 'CÂMERA' : 'AO VIVO'}
                </span>

                {thumb && <span className="absolute right-2 bottom-1.5 left-2 truncate text-[11px] text-white drop-shadow">{tile.name}</span>}
            </div>

            {! thumb && (
                <figcaption className={full ? `absolute inset-x-0 bottom-0 z-10 flex items-center gap-2 bg-[rgba(16,13,26,0.85)] px-3 py-2 transition-opacity duration-200 ${idle ? 'pointer-events-none opacity-0' : ''}` : 'flex items-center gap-2'}>
                    <Avatar name={tile.name} size={20} mine={tile.self} />
                    <span className="min-w-0 truncate text-[12px] font-medium">{tile.name}</span>

                    {! camera && stats && (
                        <span className="flex flex-none items-center gap-1.5 font-mono text-[9.5px] text-ink-dim" title={statsTitle}>
                            {stats.paused
                                ? 'pausado'
                                : (
                                    <>
                                        <span>{stats.height ? `${stats.height}p` : '—'}</span>
                                        <span className="text-ink-ghost">·</span>
                                        <span>{stats.fps} fps</span>
                                        <span className="text-ink-ghost">·</span>
                                        <span className={lossHigh ? 'text-danger' : ''}>{stats.loss === null ? '--' : `${stats.loss.toFixed(1).replace('.', ',')}%`}</span>
                                    </>
                                )}
                        </span>
                    )}

                    <span className="ml-auto flex flex-none items-center gap-1">
                        {! camera && ! native && volume !== null && (
                            <button className={`${TILE_BUTTON} ${muted ? 'border-danger/35 bg-danger/10 text-danger' : ''}`} type="button" title={muted ? 'Ativar o áudio desta tela' : 'Mutar o áudio desta tela'} onClick={() => media.toggleAudioMute(tile.key)}>
                                <Icon name={muted ? 'speakerOff' : 'speaker'} size={13} />
                            </button>
                        )}

                        {! camera && native && (
                            <button className={`${TILE_BUTTON} ${nativeMuted ? 'border-danger/35 bg-danger/10 text-danger' : ''}`} type="button" title={nativeMuted ? 'Ativar o áudio desta tela' : 'Mutar o áudio desta tela'} onClick={() => media.toggleNativeMute(tile.key)}>
                                <Icon name={nativeMuted ? 'speakerOff' : 'speaker'} size={13} />
                            </button>
                        )}

                        {! camera && (
                            <span className="relative">
                                <button className={`${TILE_BUTTON} ${panelOpen ? 'border-brand/60 text-ink-strong' : ''}`} type="button" title="Volume e imagem — só do seu lado, não mudam o que os outros veem" onClick={() => setPanelOpen(value => ! value)}>
                                    <Icon name="sliders" size={13} />
                                </button>
                                <Popover open={panelOpen} onClose={() => setPanelOpen(false)} className="right-0 bottom-8 w-60 p-3">
                                    {! native && volume !== null && (
                                        <label className="mb-3 flex flex-col gap-1 text-[11.5px] text-ink-soft">
                                            Volume {muted ? '(mudo)' : `${volume}%`}
                                            <input className="accent-brand" type="range" min="0" max="100" value={muted ? 0 : volume} onChange={event => media.setVolume(tile.key, Number(event.target.value))} />
                                        </label>
                                    )}
                                    {imageControls.map(([property, label, value]) => (
                                        <label key={property} className="mb-2.5 flex flex-col gap-1 text-[11.5px] text-ink-soft">
                                            {label}
                                            <input className="accent-brand" type="range" min="50" max="250" value={value} onChange={event => media.setImage(property, Number(event.target.value))} />
                                        </label>
                                    ))}
                                    <button className="cursor-pointer text-[11.5px] text-ink-dim hover:text-ink-strong" type="button" onClick={() => media.resetImage()}>Voltar ao padrão</button>
                                </Popover>
                            </span>
                        )}

                        {! camera && ! native && (
                            <button className={TILE_BUTTON} type="button" title={paused ? 'Retomar' : 'Pausar: para de receber sem sair da sala'} onClick={() => void media.togglePause(tile.key)}>
                                <Icon name={paused ? 'play' : 'pause'} size={12} />
                            </button>
                        )}

                        <button className={`${TILE_BUTTON} ${focused ? 'border-transparent bg-gradient-to-b from-brand to-brand-dark text-ink-strong' : ''}`} type="button" title={focused ? 'Sair do foco' : 'Focar esta tela'} onClick={() => media.focus(tile.key)}>
                            <Icon name="focus" size={12} />
                        </button>

                        {! camera && (
                            <button className={TILE_BUTTON} type="button" title={full ? 'Sair da tela cheia' : 'Tela cheia'} onClick={() => void media.toggleFullscreen(tile.key)}>
                                <Icon name={full ? 'fullscreenExit' : 'fullscreen'} size={12} />
                            </button>
                        )}

                        {! (tile.self && camera) && (
                            <button className={`${TILE_BUTTON} hover:border-danger/45 hover:text-danger`} type="button" title="Fechar esta transmissão (continua ao vivo para os outros)" onClick={() => void media.closeTile(tile.key)}>
                                <Icon name="close" size={12} />
                            </button>
                        )}
                    </span>
                </figcaption>
            )}
        </figure>
    );

    return full ? createPortal(figure, document.body) : figure;
});
