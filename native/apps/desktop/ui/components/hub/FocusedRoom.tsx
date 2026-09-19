import { RoomToolbar } from '../room/RoomToolbar.tsx';
import { Stage } from '../room/Stage.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { ChatPanel } from './ChatPanel.tsx';

export function FocusedRoom() {
    const hub = useApp().hub;
    const { can, channel: voiceChannel } = useStore(hub.voice.store);
    const { channel, stageChat } = useStore(hub.store);

    return (
        <div className="flex min-w-0 flex-1 animate-fade-in gap-3">
            <div className="flex min-w-0 flex-1 flex-col gap-3">
                <RoomToolbar mode="voice" />
                <Stage canShare={can.includes('stream')} />
            </div>

            {stageChat && (
                <div className="flex w-[320px] flex-none animate-rise flex-col">
                    <ChatPanel
                        key={stageChat}
                        chat={stageChat === 'voice' ? hub.voiceChat : hub.chat}
                        channel={stageChat === 'voice' ? voiceChannel : channel}
                        onClose={() => hub.setStageChat(null)}
                    />
                </div>
            )}
        </div>
    );
}
