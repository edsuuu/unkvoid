import { useEffect, useRef, type ReactNode } from 'react';

import { Icon } from './Icon.tsx';

type ModalProps = {
    title: string;
    subtitle?: string | null;
    leading?: ReactNode;
    width?: number;
    onClose?: () => void;
    footer?: ReactNode;
    children: ReactNode;
};

export function Modal({ title, subtitle = null, leading = null, width = 480, onClose, footer = null, children }: ModalProps) {
    const panel = useRef<HTMLDivElement>(null);
    const opener = useRef(document.activeElement as HTMLElement | null);

    useEffect(() => {
        const previous = opener.current;

        if (! panel.current?.contains(document.activeElement)) {
            panel.current?.focus();
        }

        return () => previous?.focus?.();
    }, []);

    return (
        <div
            className="fixed inset-0 z-50 flex animate-fade-in items-center justify-center bg-[rgba(6,5,10,0.74)] p-6 backdrop-blur-sm"
            onMouseDown={event => event.target === event.currentTarget && onClose?.()}
        >
            <div ref={panel} tabIndex={-1} className="glass-panel flex max-h-full w-full animate-rise flex-col outline-none" style={{ maxWidth: width }} role="dialog" aria-modal="true" aria-label={title}>
                <div className="flex items-start gap-3 px-6 pt-6">
                    {leading}
                    <div className="min-w-0 flex-1">
                        <h2 className="m-0 text-[18px] font-semibold tracking-tight">{title}</h2>
                        {subtitle && <p className="mt-1 text-[12.5px] text-ink-soft">{subtitle}</p>}
                    </div>
                    {onClose && (
                        <button className="cursor-pointer rounded-md p-1 text-ink-dim hover:text-ink-strong" type="button" onClick={onClose} title="Fechar">
                            <Icon name="close" />
                        </button>
                    )}
                </div>
                <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-6 py-5">{children}</div>
                {footer && <div className="flex flex-wrap items-center gap-2 border-t border-line px-6 py-4">{footer}</div>}
            </div>
        </div>
    );
}
