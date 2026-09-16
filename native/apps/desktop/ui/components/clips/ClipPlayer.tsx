import { useEffect, useRef } from 'react';

import type { Clip } from '../../core/Models.ts';
import { useApp } from '../useApp.ts';

export function ClipPlayer({ clip }: { clip: Clip }) {
    const clips = useApp().hub.clips;
    const video = useRef<HTMLVideoElement>(null);

    useEffect(() => {
        const element = video.current;

        clips.attach(element);

        return () => clips.detach(element);
    }, [clips, clip.id]);

    return (
        <div className="glass sticky top-0 z-10 animate-rise p-3">
            <div className="mb-2 flex items-center gap-2">
                <p className="min-w-0 flex-1 truncate text-[13px] font-semibold">{clip.streamer.name} · {clip.server_name} › {clip.channel_name}</p>
                <button className="btn-ghost px-3 py-1.5 text-[12px]" type="button" onClick={() => clips.closePlayer()}>Fechar</button>
            </div>
            <video ref={video} className="aspect-video max-h-[60vh] w-full rounded-xl bg-black" controls playsInline />
        </div>
    );
}
