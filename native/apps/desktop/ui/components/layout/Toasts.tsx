import { Icon } from '../common/Icon.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function Toasts() {
    const app = useApp();
    const { toasts } = useStore(app.store);

    return (
        <div className="pointer-events-none fixed bottom-5 left-1/2 z-[70] flex -translate-x-1/2 flex-col items-center gap-2" aria-live="polite">
            {toasts.map(toast => (
                <div
                    key={toast.id}
                    className={`pointer-events-auto flex max-w-md animate-rise items-center gap-2.5 rounded-xl border bg-[rgba(16,13,26,0.94)] px-4 py-2.5 text-[13px] text-ink-body shadow-[0_24px_60px_-24px_rgba(0,0,0,0.95)] backdrop-blur-xl ${toast.error ? 'border-danger/45' : 'border-brand/35'}`}
                >
                    <span className={toast.error ? 'text-danger' : 'text-online'}>
                        <Icon name={toast.error ? 'close' : 'check'} size={14} />
                    </span>
                    <span className="min-w-0">{toast.message}</span>
                    <button className="-my-1 ml-1 flex size-7 flex-none cursor-pointer items-center justify-center rounded-md text-ink-dim hover:bg-row hover:text-ink-strong" type="button" title="Dispensar" onClick={() => app.dismissToast(toast.id)}>
                        <Icon name="close" size={12} />
                    </button>
                </div>
            ))}
        </div>
    );
}
