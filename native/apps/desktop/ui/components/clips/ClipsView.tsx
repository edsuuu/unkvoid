import { Icon } from '../common/Icon.tsx';
import { AuthCard } from '../entry/AuthCard.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { ClipCard } from './ClipCard.tsx';
import { ClipPlayer } from './ClipPlayer.tsx';

export function ClipsView() {
    const app = useApp();
    const hub = app.hub;
    const { user } = useStore(hub.store);
    const { clips, loading, failed, playing } = useStore(hub.clips.store);

    if (! user) {
        return (
            <div className="scroll-thin flex h-full flex-col items-center justify-center gap-4 overflow-y-auto p-6">
                <p className="text-[13px] text-ink-soft">Os clipes são da sua conta: entre para ver os seus.</p>
                <AuthCard />
            </div>
        );
    }

    return (
        <div className="scroll-thin flex h-full flex-col gap-4 overflow-y-auto p-5">
            <div className="animate-rise flex items-start gap-3">
                <button className="btn-icon mt-0.5 size-8 flex-none rounded-[10px]" type="button" title="Voltar para a transmissão" onClick={() => app.setTab('broadcast')}>
                    <Icon name="arrowLeft" size={16} />
                </button>
                <div className="min-w-0">
                <p className="text-[19px] font-semibold tracking-tight">Seus clipes</p>
                <p className="mt-1 text-[13px] text-ink-soft">Os últimos 5 minutos de quem compartilhava a tela na voz. Só você vê os seus, e cada um some em 7 dias.</p>
                </div>
            </div>

            {playing && <ClipPlayer key={playing.id} clip={playing} />}

            {loading && clips.length === 0 && (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-3">
                    {[0, 1, 2].map(index => <div key={index} className="skeleton aspect-[4/3] rounded-[20px]" />)}
                </div>
            )}

            {! loading && clips.length === 0 && failed && (
                <div className="glass mx-auto mt-6 flex max-w-md flex-col items-center gap-3 p-8 text-center">
                    <p className="text-[14px] text-danger">Não deu para carregar os seus clipes.</p>
                    <button className="btn-ghost" type="button" onClick={() => void hub.clips.load()}>Tentar de novo</button>
                </div>
            )}

            {! loading && clips.length === 0 && ! failed && (
                <div className="glass mx-auto mt-6 max-w-md p-8 text-center">
                    <p className="text-[15px] font-semibold">Nenhum clipe ainda.</p>
                    <p className="mt-2 text-[13px] text-ink-soft">Na voz, quando alguém compartilha a tela, o Clipar guarda os últimos 5 minutos.</p>
                </div>
            )}

            <div className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-3">
                {clips.map(clip => <ClipCard key={clip.id} clip={clip} />)}
            </div>
        </div>
    );
}
