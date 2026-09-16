import { useState } from 'react';

import { RoomToolbar } from '../room/RoomToolbar.tsx';
import { Stage } from '../room/Stage.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { ChatPanel } from './ChatPanel.tsx';

export function FocusedRoom() {
    const hub = useApp().hub;
    const { can } = useStore(hub.voice.store);
    const [chatOpen, setChatOpen] = useState(false);

    return (
        <div className="flex min-w-0 flex-1 animate-fade-in gap-3">
            <div className="flex min-w-0 flex-1 flex-col gap-3">
                <RoomToolbar mode="voice" chatOpen={chatOpen} onToggleChat={() => setChatOpen(open => ! open)} />
                <Stage canShare={can.includes('stream')} />
            </div>

            {chatOpen && (
                <div className="flex w-[320px] flex-none animate-rise flex-col">
                    <ChatPanel onClose={() => setChatOpen(false)} />
                </div>
            )}
        </div>
    );
}
