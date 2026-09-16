import { Avatar } from '../common/Avatar.tsx';
import { Icon } from '../common/Icon.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function ServerRail() {
    const hub = useApp().hub;
    const { servers, serversLoading, tree, home, railOpen } = useStore(hub.store);
    const label = 'min-w-0 flex-1 truncate whitespace-nowrap text-left text-[13px] text-ink-body';

    return (
        <nav
            className="glass scroll-thin flex flex-none flex-col gap-2 overflow-y-auto overflow-x-hidden rounded-[18px] py-2 transition-[width,padding] duration-200"
            style={{ width: railOpen ? 182 : 58, paddingLeft: railOpen ? 8 : 12, paddingRight: railOpen ? 8 : 12 }}
        >
            <button className="btn-icon size-[34px] flex-none self-start" type="button" title={railOpen ? 'Recolher servidores' : 'Expandir servidores'} onClick={() => hub.toggleRail()}>
                <Icon name="menu" size={15} />
            </button>

            <button className="flex w-full cursor-pointer items-center gap-2.5" type="button" title="Home — criar sala e últimas salas" onClick={() => hub.showHome()}>
                <span className={`btn-icon ${home || ! tree ? 'btn-icon-on' : ''}`}><Icon name="home" size={16} /></span>
                <span className={label}>Home</span>
            </button>

            <span className="h-px w-full flex-none bg-line-strong" />

            {serversLoading && servers.length === 0 && [0, 1, 2].map(index => <span key={index} className="skeleton size-[34px] flex-none rounded-[11px]" />)}

            {servers.map(server => {
                const active = ! home && tree?.id === server.id;

                return (
                    <button key={server.id} className="group flex w-full cursor-pointer items-center gap-2.5" type="button" title={server.name} onClick={() => void hub.attempt(() => hub.openServer(server.id))}>
                        <span className={`rounded-[11px] transition ${active ? '' : 'ring-1 ring-white/[0.08] group-hover:ring-brand/40'}`}>
                            <Avatar name={server.name} url={server.icon_url} size={34} mine={active} square />
                        </span>
                        <span className={label}>{server.name}</span>
                    </button>
                );
            })}

            <button className="flex w-full cursor-pointer items-center gap-2.5" type="button" title="Criar servidor ou entrar com convite" onClick={() => hub.openModal({ type: 'server' })}>
                <span className="flex size-[34px] flex-none items-center justify-center rounded-[11px] border border-dashed border-white/20 text-ink-dim transition hover:border-brand hover:text-ink-strong">
                    <Icon name="plus" size={15} />
                </span>
                <span className={label}>Criar servidor</span>
            </button>
        </nav>
    );
}
