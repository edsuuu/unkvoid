import { Icon } from '../common/Icon.tsx';
import { Popover } from '../common/Popover.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function ClipButton({ wide = false }: { wide?: boolean }) {
    const app = useApp();
    const voice = app.hub.voice;
    const { clipOpen } = useStore(voice.store);

    useStore(app.media.store);
    useStore(app.sharing.store);

    const streamers = voice.streamers();

    if (streamers.length === 0) {
        return null;
    }

    return (
        <span className={`relative ${wide ? 'flex' : 'flex-none'}`}>
            <button
                className={wide ? `btn-ghost flex h-8 w-full items-center justify-center gap-1.5 rounded-[9px] p-0 ${clipOpen ? 'border-brand/60 text-ink-strong' : ''}` : `btn-icon ${clipOpen ? 'btn-icon-on' : ''}`}
                type="button"
                title="Guardar os últimos 5 minutos de quem está compartilhando a tela"
                onClick={() => voice.toggleClipList()}
            >
                <Icon name="scissors" size={15} />
                {wide && <span className="text-[11.5px]">Clipar</span>}
            </button>

            <Popover open={clipOpen} onClose={() => voice.store.set({ clipOpen: false })} className={wide ? 'bottom-10 left-0 w-56' : 'top-11 right-0 w-56'}>
                <p className="label-mono px-2 py-1.5">Clipar os últimos 5 min de</p>
                {streamers.map(streamer => (
                    <button key={streamer.userId} className="flex w-full cursor-pointer items-center gap-2 rounded-[10px] px-2.5 py-2 text-left text-[12.5px] text-ink-icon hover:bg-row hover:text-ink-strong" type="button" onClick={() => void voice.clip(streamer)}>
                        <Icon name="scissors" size={13} />
                        <span className="truncate">{streamer.name}</span>
                    </button>
                ))}
            </Popover>
        </span>
    );
}
