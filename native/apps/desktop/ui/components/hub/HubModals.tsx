import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { ChannelModal } from './modals/ChannelModal.tsx';
import { MemberMenu } from './modals/MemberMenu.tsx';
import { RoleModal } from './modals/RoleModal.tsx';
import { ServerModal } from './modals/ServerModal.tsx';
import { ServerSettingsModal } from './modals/ServerSettingsModal.tsx';
import { UserSettingsModal } from './modals/UserSettingsModal.tsx';

export function HubModals() {
    const hub = useApp().hub;
    const { modal, roleEditor, memberMenu, tree, user } = useStore(hub.store);

    return (
        <>
            {modal?.type === 'server' && <ServerModal />}
            {modal?.type === 'settings' && tree && <ServerSettingsModal />}
            {modal?.type === 'channel' && tree && <ChannelModal key={modal.channel?.id ?? modal.channelType} channel={modal.channel} channelType={modal.channelType} />}
            {modal?.type === 'user' && user && <UserSettingsModal />}
            {roleEditor && tree && <RoleModal key={roleEditor.role?.id ?? 'new'} role={roleEditor.role} />}
            {memberMenu && tree && <MemberMenu key={memberMenu.userId} />}
        </>
    );
}
