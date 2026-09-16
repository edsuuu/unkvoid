import { Spinner } from '../common/Spinner.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function UpdateScreen() {
    const app = useApp();
    const { updateStatus, updateProgress } = useStore(app.store);

    return (
        <section className="fixed inset-0 z-50 flex animate-fade-in flex-col items-center justify-center gap-4 bg-back">
            <h1 className="m-0 text-2xl font-semibold tracking-tight">Unkvoid</h1>

            {updateProgress === null
                ? <Spinner size={18} />
                : (
                    <div className="h-1.5 w-56 overflow-hidden rounded-full bg-row">
                        <div className="h-full rounded-full bg-gradient-to-r from-brand to-online transition-[width] duration-150" style={{ width: `${updateProgress}%` }} />
                    </div>
                )}

            <p className="text-[13px] text-ink-soft">{updateStatus}</p>
        </section>
    );
}
