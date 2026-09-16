import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { FocusedRoom } from './FocusedRoom.tsx';
import { HomeView } from './HomeView.tsx';
import { ServerRail } from './ServerRail.tsx';
import { ServerView } from './ServerView.tsx';

export function HubScreen() {
    const hub = useApp().hub;
    const { tree, home, focusedRoom, treeLoading } = useStore(hub.store);
    const { channel: voiceChannel } = useStore(hub.voice.store);

    if (focusedRoom && voiceChannel && ! home) {
        return (
            <div className="flex h-full gap-3 p-3">
                <FocusedRoom />
            </div>
        );
    }

    return (
        <div className="flex h-full gap-3 p-3">
            <ServerRail />
            {treeLoading && (
                <div className="flex min-w-0 flex-1 animate-fade-in gap-3">
                    <div className="flex w-[300px] flex-none flex-col gap-3">
                        <div className="glass p-4"><div className="skeleton h-6 w-40" /></div>
                        <div className="glass flex flex-1 flex-col gap-2.5 p-4">
                            {[0, 1, 2, 3].map(index => <div key={index} className="skeleton h-9" />)}
                        </div>
                        <div className="glass p-3"><div className="skeleton h-9" /></div>
                    </div>
                    <div className="glass flex-1 p-5"><div className="skeleton h-6 w-32" /></div>
                </div>
            )}
            {! treeLoading && (home || ! tree ? <HomeView /> : <ServerView />)}
        </div>
    );
}
