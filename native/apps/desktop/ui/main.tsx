import { createRoot } from 'react-dom/client';

import { AppContext } from './components/AppContext.ts';
import { AppShell } from './components/AppShell.tsx';
import { ErrorBoundary } from './components/ErrorBoundary.tsx';
import { App } from './core/App.ts';
import { Desktop } from './core/Desktop.ts';
import { Failure } from './core/Failure.ts';
import { Tauri } from './core/Tauri.ts';
import { DevTauriBridge } from './dev/DevTauriBridge.ts';

declare global {
    interface Window {
        unkvoid?: App;
    }
}

DevTauriBridge.install();

if (Tauri.available()) {
    Desktop.harden();

    const app = new App();

    window.unkvoid = app;

    window.addEventListener('unhandledrejection', event => app.log('ui.unhandled', { message: Failure.message(event.reason), stack: (event.reason as Error | null)?.stack ?? null }));
    window.addEventListener('error', event => app.log('ui.unhandled', { message: event.message, source: `${event.filename}:${event.lineno}` }));

    createRoot(document.getElementById('root')!).render(
        <AppContext.Provider value={app}>
            <ErrorBoundary>
                <AppShell />
            </ErrorBoundary>
        </AppContext.Provider>,
    );

    void app.start();
} else {
    const message = document.getElementById('boot-error')!;

    message.textContent = 'Build quebrada: a ponte do Tauri não carregou.';
    message.hidden = false;
}
