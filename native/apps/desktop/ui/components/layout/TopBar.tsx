import { useState } from 'react';

import { Platform } from '../../core/Platform.ts';
import { Avatar } from '../common/Avatar.tsx';
import { Icon } from '../common/Icon.tsx';
import { Popover } from '../common/Popover.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

const MENU_ITEM_BASE = 'flex w-full cursor-pointer items-center gap-2.5 rounded-[9px] px-2.5 py-2 text-left text-[12.5px] hover:bg-row';
const MENU_ITEM = `${MENU_ITEM_BASE} text-ink-icon hover:text-ink-strong`;

export function TopBar() {
    const app = useApp();
    const hub = app.hub;
    const { tab, screen } = useStore(app.store);
    const { user, connected } = useStore(hub.store);
    const [menuOpen, setMenuOpen] = useState(false);

    const choose = (work: () => unknown) => {
        setMenuOpen(false);
        void work();
    };

    return (
        <header className="relative z-30 flex h-12 flex-none items-center gap-3 border-b border-line bg-[rgba(16,13,26,0.62)] px-4 backdrop-blur-xl">
            <span className="flex items-center gap-2 text-[14px] font-semibold tracking-tight">
                <span className="size-2 rounded-full bg-brand shadow-[0_0_12px_rgba(138,124,245,0.8)]" />
                Unkvoid
            </span>

            <nav className="ml-2 flex gap-1.5" role="tablist">
                <button className={`tab-pill ${tab === 'broadcast' ? 'tab-pill-on' : ''}`} type="button" role="tab" aria-selected={tab === 'broadcast'} onClick={() => app.setTab('broadcast')}>Transmissão</button>
                {Platform.isWindows() && <button className={`tab-pill ${tab === 'clips' ? 'tab-pill-on' : ''}`} type="button" role="tab" aria-selected={tab === 'clips'} onClick={() => app.setTab('clips')}>Clipes</button>}
            </nav>

            <span className="flex-1" />

            {user && ! connected && (
                <span className="flex items-center gap-2 font-mono text-[10.5px] text-danger">
                    <span className="size-1.5 animate-pulse rounded-full bg-danger" />
                    reconectando…
                </span>
            )}

            {user && (
                <button className="cursor-pointer rounded-full" type="button" title="Configurações da conta" onClick={() => hub.openModal({ type: 'user' })}>
                    <Avatar name={user.name} size={28} mine />
                </button>
            )}

            <span className="relative">
                <button className={`btn-icon size-8 ${menuOpen ? 'btn-icon-on' : ''}`} type="button" title="Menu" onClick={() => setMenuOpen(open => ! open)}>
                    <Icon name="menu" />
                </button>
                <Popover open={menuOpen} onClose={() => setMenuOpen(false)} className="top-10 right-0 w-60">
                    <div className="mb-1 border-b border-line px-2.5 pt-1 pb-2">
                        <p className="truncate text-[13px] font-semibold">{user?.name ?? 'Sem conta'}</p>
                        <p className="label-mono mt-0.5">{user ? 'conta conectada' : 'usando sem login'}</p>
                    </div>
                    {user && <button className={MENU_ITEM} type="button" onClick={() => choose(() => hub.openModal({ type: 'user' }))}><Icon name="gear" size={15} />Configurações da conta</button>}
                    {user && screen === 'hub' && <button className={MENU_ITEM} type="button" onClick={() => choose(() => hub.roomByCode())}><Icon name="hash" size={15} />Sala por código</button>}
                    <button className={MENU_ITEM} type="button" onClick={() => choose(() => app.openLogs())}><Icon name="logs" size={15} />Logs</button>
                    {user && <button className={`${MENU_ITEM_BASE} text-periwinkle`} type="button" onClick={() => choose(() => hub.logout())}><Icon name="logout" size={15} />Sair da conta</button>}
                </Popover>
            </span>
        </header>
    );
}
