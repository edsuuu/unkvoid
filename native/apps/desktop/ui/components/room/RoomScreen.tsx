import { Icon } from '../common/Icon.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';
import { RoomToolbar } from './RoomToolbar.tsx';
import { Stage } from './Stage.tsx';

export function RoomScreen() {
    const app = useApp();
    const { room, roomError } = useStore(app.store);

    return (
        <div className="flex h-full flex-col gap-3 p-3">
            <RoomToolbar mode="code" />

            {roomError && (
                <div className="flex animate-rise items-center gap-3 rounded-xl border border-danger/35 bg-danger/10 px-4 py-2.5 text-[13px] text-danger" role="alert">
                    <span className="min-w-0 flex-1">{roomError}</span>
                    <button className="flex size-7 flex-none cursor-pointer items-center justify-center rounded-md hover:bg-danger/15" type="button" title="Dispensar" onClick={() => app.dismissRoomError()}>
                        <Icon name="close" size={14} />
                    </button>
                </div>
            )}

            <Stage canShare hint={<>Mande o código <code className="code-chip text-[12px]">{room}</code> para quem você quer aqui.</>} />
        </div>
    );
}
