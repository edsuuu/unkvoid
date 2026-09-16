import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function OfflineScreen() {
    const app = useApp();
    const { offlineTitle, offlineStatus } = useStore(app.store);

    return (
        <section className="fixed inset-0 z-50 flex animate-fade-in flex-col items-center justify-center gap-3 bg-back px-6 text-center">
            <span className="relative mb-2 size-3">
                <span className="absolute inset-0 animate-ping-soft rounded-full bg-danger" />
                <span className="absolute inset-0 rounded-full bg-danger" />
            </span>
            <h1 className="m-0 text-2xl font-semibold tracking-tight">{offlineTitle}</h1>
            <p className="text-[13px] text-ink-soft">{offlineStatus}</p>
        </section>
    );
}
