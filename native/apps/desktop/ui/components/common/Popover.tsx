import { useEffect, useRef, type ReactNode } from 'react';

type PopoverProps = {
    open: boolean;
    onClose: () => void;
    className?: string;
    children: ReactNode;
};

export function Popover({ open, onClose, className = '', children }: PopoverProps) {
    const box = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (! open) {
            return undefined;
        }

        const closeOutside = (event: MouseEvent) => {
            if (box.current && ! box.current.parentElement!.contains(event.target as Node)) {
                onClose();
            }
        };
        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key !== 'Escape') {
                return;
            }

            event.preventDefault();
            onClose();
        };

        document.addEventListener('mousedown', closeOutside);
        document.addEventListener('keydown', closeOnEscape, true);

        return () => {
            document.removeEventListener('mousedown', closeOutside);
            document.removeEventListener('keydown', closeOnEscape, true);
        };
    }, [open, onClose]);

    if (! open) {
        return null;
    }

    return (
        <div ref={box} className={`popover absolute z-40 animate-rise p-2 ${className}`}>
            {children}
        </div>
    );
}
