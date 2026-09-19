import { useEffect } from 'react';
import { createPortal } from 'react-dom';

import { Icon } from './Icon.tsx';

export function Lightbox({ url, onClose }: { url: string; onClose: () => void }) {
    useEffect(() => {
        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key !== 'Escape') {
                return;
            }

            event.preventDefault();
            event.stopPropagation();
            onClose();
        };

        document.addEventListener('keydown', closeOnEscape, true);

        return () => document.removeEventListener('keydown', closeOnEscape, true);
    }, [onClose]);

    return createPortal(
        <div className="fixed inset-0 z-[60] flex animate-fade-in items-center justify-center bg-[rgba(6,5,10,0.88)] p-6" role="dialog" aria-modal="true" aria-label="Imagem ampliada" onClick={onClose}>
            <img className="max-h-full max-w-full rounded-lg object-contain" src={url} alt="Imagem ampliada" onClick={event => event.stopPropagation()} />
            <button className="btn-icon absolute top-4 right-4" type="button" title="Fechar" onClick={onClose}>
                <Icon name="close" size={16} />
            </button>
        </div>,
        document.body,
    );
}
