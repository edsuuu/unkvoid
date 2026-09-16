import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { DirectColumn } from './home/DirectColumn.tsx';
import { DirectPanel } from './home/DirectPanel.tsx';
import { FriendsPanel } from './home/FriendsPanel.tsx';
import { ServersHome } from './home/ServersHome.tsx';

export function HomeView() {
    const hub = useApp().hub;
    const { homeTab } = useStore(hub.store);
    const { person } = useStore(hub.direct.store);

    return (
        <div className="flex min-w-0 flex-1 gap-3">
            <DirectColumn />
            {person ? <DirectPanel /> : homeTab === 'friends' ? <FriendsPanel /> : <ServersHome />}
        </div>
    );
}
