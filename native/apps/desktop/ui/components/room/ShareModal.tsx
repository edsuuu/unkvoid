import { Platform } from '../../core/Platform.ts';
import { Sharing } from '../../core/Sharing.ts';
import { Modal } from '../common/Modal.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function ShareModal() {
    const app = useApp();
    const sharing = app.sharing;
    const { open, loading, tab, sources, previews, source, audio, muteCalls, quality, fps, active } = useStore(sharing.store);

    if (! open) {
        return null;
    }

    const items = sources[tab] ?? [];
    const linuxNote = Platform.isLinux() && audio && muteCalls;
    const emptyText = tab === 'window'
        ? 'Nenhuma janela aberta para compartilhar.'
        : Platform.isLinux()
            ? 'Nenhuma tela X11 encontrada. Em sessão Wayland a captura ainda não funciona.'
            : 'Nenhuma tela encontrada. No macOS, autorize a gravação de tela nas Configurações do Sistema.';

    return (
        <Modal
            title={active ? 'Mudar a transmissão' : 'Compartilhar tela'}
            subtitle="Escolha o que a sala vai ver."
            width={560}
            onClose={() => sharing.close()}
            footer={(
                <>
                    <label className="flex flex-col gap-1.5">
                        <span className="label-mono">Qualidade</span>
                        <select className="field cursor-pointer py-2 text-[13px]" value={quality} onChange={event => sharing.setQuality(event.target.value)}>
                            {Sharing.QUALITIES.map(value => <option key={value} value={value}>{value === '2160' ? '4K (2160p)' : `${value}p`}</option>)}
                        </select>
                    </label>
                    <label className="flex flex-col gap-1.5">
                        <span className="label-mono">FPS</span>
                        <select className="field cursor-pointer py-2 text-[13px]" value={fps} onChange={event => sharing.setFps(event.target.value)}>
                            {Sharing.FRAME_RATES.map(value => <option key={value} value={value}>{value}</option>)}
                        </select>
                    </label>
                    <span className="flex-1" />
                    <button className="cursor-pointer self-end px-3.5 py-2.5 text-[13.5px] text-ink-dim transition hover:text-ink-strong" type="button" onClick={() => sharing.close()}>Cancelar</button>
                    <button className="btn-primary self-end rounded-[10px] px-[18px] py-[11px] text-[13.5px] font-semibold" type="button" disabled={! source} onClick={() => void sharing.confirm()}>Transmitir</button>
                </>
            )}
        >
            <div className="mb-4 flex gap-1.5">
                <button className={`tab-soft ${tab === 'display' ? 'tab-soft-on' : ''}`} type="button" onClick={() => sharing.setTab('display')}>Telas</button>
                <button className={`tab-soft ${tab === 'window' ? 'tab-soft-on' : ''}`} type="button" onClick={() => sharing.setTab('window')}>Aplicativos</button>
            </div>

            <div className="scroll-thin h-[340px] overflow-y-auto pr-1">
                {loading && (
                    <div className="grid grid-cols-2 gap-3">
                        {[0, 1].map(index => <div key={index} className="skeleton aspect-[16/12] rounded-[14px]" />)}
                    </div>
                )}

                {! loading && items.length === 0 && <p className="text-[13px] text-ink-soft">{emptyText}</p>}

                {! loading && items.length > 0 && (
                    <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,150px),1fr))] gap-3">
                        {items.map(item => (
                            <button
                                key={item.value}
                                className={`cursor-pointer overflow-hidden rounded-[14px] border bg-white/[0.03] text-left transition ${source === item.value ? 'border-brand/45' : 'border-white/[0.09] hover:border-brand/30'}`}
                                type="button"
                                onClick={() => sharing.pick(item.value)}
                            >
                                <div className="flex aspect-[16/10] items-center justify-center bg-gradient-to-br from-brand-dark/30 to-black">
                                    {previews[item.value]
                                        ? <img className="size-full object-contain" src={previews[item.value]} alt="" />
                                        : <span className="font-mono text-[10px] text-ink-dim">sem prévia</span>}
                                </div>
                                <div className="px-3 py-2.5">
                                    <p className="truncate text-[13.5px] font-medium">{item.label}</p>
                                    <p className="mt-0.5 truncate font-mono text-[11px] text-ink-dim">{item.detail}</p>
                                </div>
                            </button>
                        ))}
                    </div>
                )}
            </div>

            <div className="mt-5 flex flex-wrap gap-x-5 gap-y-2.5 text-[13px] text-ink-icon">
                <label className="flex cursor-pointer items-center gap-2">
                    <input className="size-4 cursor-pointer accent-brand" type="checkbox" checked={audio} onChange={event => sharing.setAudio(event.target.checked)} />
                    Transmitir o áudio
                </label>
                <label className={`flex items-center gap-2 ${audio ? 'cursor-pointer' : 'opacity-50'}`}>
                    <input className="size-4 cursor-pointer accent-brand" type="checkbox" checked={muteCalls} disabled={! audio} onChange={event => sharing.setMuteCalls(event.target.checked)} />
                    Sem o áudio do Discord
                </label>
                {linuxNote && <span className="basis-full text-[12px] text-ink-dim">No Linux vai o som do sistema inteiro: não dá para deixar o Discord de fora.</span>}
            </div>
        </Modal>
    );
}
