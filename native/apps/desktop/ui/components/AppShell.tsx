import { ClipsView } from './clips/ClipsView.tsx';
import { EntryScreen } from './entry/EntryScreen.tsx';
import { HubModals } from './hub/HubModals.tsx';
import { HubScreen } from './hub/HubScreen.tsx';
import { ConfirmDialog } from './layout/ConfirmDialog.tsx';
import { LogsModal } from './layout/LogsModal.tsx';
import { OfflineScreen } from './layout/OfflineScreen.tsx';
import { Toasts } from './layout/Toasts.tsx';
import { TopBar } from './layout/TopBar.tsx';
import { UpdateScreen } from './layout/UpdateScreen.tsx';
import { RoomScreen } from './room/RoomScreen.tsx';
import { ShareModal } from './room/ShareModal.tsx';
import { useApp } from './useApp.ts';
import { useStore } from './useStore.ts';

export function AppShell() {
    const app = useApp();
    const { screen, tab } = useStore(app.store);

    return (
        <div className="flex h-screen flex-col overflow-hidden">
            {screen === 'update' && <UpdateScreen />}
            {screen === 'offline' && <OfflineScreen />}

            {['entry', 'room', 'hub'].includes(screen) && (
                <>
                    <TopBar />
                    <main className="relative min-h-0 flex-1">
                        <div className="h-full" hidden={tab !== 'broadcast'}>
                            {screen === 'entry' && <EntryScreen />}
                            {screen === 'room' && <RoomScreen />}
                            {screen === 'hub' && <HubScreen />}
                        </div>
                        {tab === 'clips' && <ClipsView />}
                    </main>
                </>
            )}

            <HubModals />
            <ShareModal />
            <LogsModal />
            <ConfirmDialog />
            <Toasts />
        </div>
    );
}
