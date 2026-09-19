import { useCallback, useState } from 'react';

import type { Chat } from '../../core/Chat.ts';
import type { Message } from '../../core/Models.ts';
import { Lightbox } from '../common/Lightbox.tsx';

export function MessageImages({ chat, message }: { chat: Chat; message: Message }) {
    const [enlarged, setEnlarged] = useState<number | null>(null);
    const close = useCallback(() => setEnlarged(null), []);
    const files = message.files ?? [];
    const open = files.find(file => file.id === enlarged);
    const single = files.length === 1;

    if (files.length === 0) {
        return null;
    }

    return (
        <div className="mt-1.5 flex max-w-full flex-wrap gap-1.5">
            {files.map(file => (
                <button key={file.id} className="max-w-full cursor-zoom-in overflow-hidden rounded-[10px] border border-line-strong bg-row" type="button" title="Ampliar a imagem" onClick={() => setEnlarged(file.id)}>
                    <img
                        className={single ? 'block max-h-[320px] max-w-full object-contain' : 'block h-[150px] w-[150px] max-w-full object-cover'}
                        src={file.url}
                        alt="Imagem enviada"
                        loading="lazy"
                        onError={() => void chat.renewFiles(message)}
                    />
                </button>
            ))}

            {open && <Lightbox url={open.url} onClose={close} />}
        </div>
    );
}
