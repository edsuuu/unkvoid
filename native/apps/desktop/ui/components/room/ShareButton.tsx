import { useState } from 'react';

import { Icon } from '../common/Icon.tsx';
import { Popover } from '../common/Popover.tsx';
import { Spinner } from '../common/Spinner.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

const MENU_ITEM_BASE = 'block w-full cursor-pointer rounded-[9px] px-2.5 py-2 text-left text-[12.5px]';
const MENU_ITEM = `${MENU_ITEM_BASE} text-ink-icon hover:bg-row hover:text-ink-strong`;

export function ShareButton({ wide = false, disabled = false }: { wide?: boolean; disabled?: boolean }) {
    const app = useApp();
    const { active, starting, line } = useStore(app.sharing.store);
    const { selfView } = useStore(app.media.store);
    const [open, setOpen] = useState(false);

    const choose = (work: () => unknown) => {
        setOpen(false);
        void work();
    };

    return (
        <>
            <span className={`relative ${wide ? 'flex' : 'flex-none'}`}>
                <button
                    className={`btn-icon ${wide ? 'h-8 w-full rounded-[9px]' : ''} ${active ? 'btn-icon-on' : ''}`}
                    type="button"
                    title={active ? 'Opções da transmissão' : 'Compartilhar tela'}
                    disabled={starting || disabled}
                    onClick={() => (active ? setOpen(value => ! value) : void app.sharing.open())}
                >
                    {starting ? <Spinner size={15} /> : <Icon name="screen" size={16} />}
                </button>

                <Popover open={open} onClose={() => setOpen(false)} className={wide ? 'bottom-10 left-0 w-64' : 'top-11 right-0 w-64'}>
                    <p className="label-mono px-2.5 pt-1.5">Você está transmitindo</p>
                    <p className="px-2.5 pt-1 pb-2 font-mono text-[10.5px] text-ink-dim">
                        {! line || line.starting ? 'começando…' : `${line.fps} fps · ${line.mbps.toFixed(1)} Mb/s · ${line.dropped} perdidos${line.encoder === 'cpu' ? ' · processador' : ''}`}
                    </p>
                    <button className={MENU_ITEM} type="button" onClick={() => choose(() => app.sharing.open())}>Mudar monitor, aplicativo ou qualidade</button>
                    <button className={MENU_ITEM} type="button" onClick={() => choose(() => app.media.toggleSelfView())}>{selfView ? 'Ocultar minha tela' : 'Ver o que a sala vê'}</button>
                    <button className={`${MENU_ITEM_BASE} mt-0.5 border-t border-line text-danger hover:bg-danger/10`} type="button" onClick={() => choose(() => app.sharing.stop())}>Parar de transmitir</button>
                </Popover>
            </span>

            {active && ! wide && (
                <button className="btn-danger flex flex-none items-center gap-2 rounded-[11px] px-3 py-2 text-[12px] font-semibold" type="button" title="Parar de transmitir" onClick={() => void app.sharing.stop()}>
                    <Icon name="stop" size={18} />
                    Parar
                </button>
            )}
        </>
    );
}
