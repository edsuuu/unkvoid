import { Clips } from '../../core/Clips.ts';
import type { Clip } from '../../core/Models.ts';
import { Icon } from '../common/Icon.tsx';
import { Spinner } from '../common/Spinner.tsx';
import { useApp } from '../useApp.ts';

export function ClipCard({ clip }: { clip: Clip }) {
    const clips = useApp().hub.clips;
    const when = [
        new Date(clip.created_at).toLocaleString('pt-BR', { dateStyle: 'short', timeStyle: 'short' }),
        clip.duration_ms ? Clips.duration(clip.duration_ms) : null,
        Clips.expiry(clip.expires_at),
    ].filter(Boolean).join(' · ');
    const ready = clip.status === 'ready' && clip.playlist_url;

    return (
        <article className="glass flex animate-rise flex-col overflow-hidden">
            <button className="relative flex aspect-video items-center justify-center overflow-hidden bg-black disabled:cursor-default" type="button" disabled={! ready} onClick={() => clips.play(clip)}>
                {clip.thumbnail_url && <img className="size-full object-cover" src={clip.thumbnail_url} alt="" loading="lazy" />}
                {clip.status === 'processing' && (
                    <span className="flex items-center gap-2 text-[12px] text-ink-soft"><Spinner size={15} />Salvando o clipe…</span>
                )}
                {clip.status === 'failed' && <span className="px-4 text-center text-[12px] text-danger">Não deu para salvar este clipe.</span>}
                {ready && (
                    <span className="absolute inset-0 flex items-center justify-center bg-black/0 text-white/0 transition hover:bg-black/40 hover:text-white">
                        <Icon name="play" size={40} />
                    </span>
                )}
            </button>

            <div className="flex flex-col gap-1 p-3.5">
                <p className="truncate text-[13.5px] font-semibold">{clip.streamer.name}</p>
                <p className="truncate text-[12px] text-ink-soft">{clip.server_name} › {clip.channel_name}</p>
                <p className="font-mono text-[10px] text-ink-dim">{when}</p>
                <div className="mt-2 flex gap-1.5">
                    {ready && <button className="btn-primary px-3 py-1.5 text-[12px]" type="button" onClick={() => clips.play(clip)}>Assistir</button>}
                    {clip.download_url && (
                        <button className="btn-ghost flex items-center gap-1.5 px-3 py-1.5 text-[12px]" type="button" onClick={() => void clips.download(clip)}>
                            <Icon name="download" size={12} />
                            Baixar
                        </button>
                    )}
                    <span className="flex-1" />
                    <button className="btn-ghost px-2.5 py-1.5 text-[12px] hover:text-danger" type="button" title="Apagar" onClick={() => void clips.remove(clip)}>
                        <Icon name="trash" size={13} />
                    </button>
                </div>
            </div>
        </article>
    );
}
