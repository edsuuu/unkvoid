import { Component, type ContextType, type ReactNode } from 'react';

import { Failure } from '../core/Failure.ts';
import { AppContext } from './AppContext.ts';
import { LogsModal } from './layout/LogsModal.tsx';

type ErrorBoundaryProps = { children: ReactNode };

type ErrorBoundaryState = { error: unknown };

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
    static contextType = AppContext;

    declare context: ContextType<typeof AppContext>;

    state: ErrorBoundaryState = { error: null };

    static getDerivedStateFromError(error: unknown): ErrorBoundaryState {
        return { error };
    }

    componentDidCatch(error: Error): void {
        this.context?.reportFatal(error);
    }

    render(): ReactNode {
        if (! this.state.error) {
            return this.props.children;
        }

        return (
            <div className="flex h-screen flex-col items-center justify-center gap-4 p-8 text-center">
                <p className="text-lg font-semibold">A interface travou.</p>
                <p className="max-w-lg text-[13px] text-ink-soft">{Failure.message(this.state.error)}</p>
                <div className="flex gap-2">
                    <button className="btn-ghost" type="button" onClick={() => void this.context?.openLogs()}>Ver logs</button>
                    <button className="btn-primary" type="button" onClick={() => location.reload()}>Recarregar</button>
                </div>
                <LogsModal />
            </div>
        );
    }
}
