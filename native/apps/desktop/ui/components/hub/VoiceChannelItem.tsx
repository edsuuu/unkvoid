import { useState } from 'react';

import type { Channel, VoicePerson } from '../../core/Models.ts';
import { Avatar } from '../common/Avatar.tsx';
import { Elapsed } from '../common/Elapsed.tsx';
import { Icon } from '../common/Icon.tsx';
import { Spinner } from '../common/Spinner.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function VoiceChannelItem({ channel, people }: { channel: Channel; people: VoicePerson[] }) {
    const app = useApp();
    const hub = app.hub;
    const { channel: voiceChannel, joining } = useStore(hub.voice.store);
    const { connectedAt, ping, peers, reconnecting } = useStore(app.media.store);
    const { active: sharing } = useStore(app.sharing.store);
    const { user, tree } = useStore(hub.store);
    const [preview, setPreview] = useState<number | null>(null);
    const here = voiceChannel?.id === channel.id;

    const watch = (person: VoicePerson) => {
        setPreview(null);

        if (! here) {
            void hub.attempt(() => hub.openChannel(channel));

            return;
        }

        hub.showStage(true);

        const peer = peers.find(item => item.userId === `user:${person.user_id}`);

        if (peer?.missing) {
            void app.media.watchPeer(peer.peerId);
        }
    };

    return (
        <div className={`rounded-xl border px-3 py-2.5 transition ${here ? 'border-brand/30 bg-brand/10' : 'border-white/[0.08] hover:bg-row'}`}>
            <div className="flex items-center gap-2">
                <button className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left" type="button" title={here ? `Ping ${ping ?? '--'} ms` : 'Entrar na voz'} onClick={() => void hub.attempt(() => hub.openChannel(channel))}>
                    <span className={here ? 'text-lilac-2' : 'text-ink-dim'}><Icon name="speaker" size={14} /></span>
                    <span className={`min-w-0 flex-1 truncate text-[13px] ${here ? 'font-medium text-ink-body' : 'text-ink-icon'}`}>{channel.name}</span>
                </button>

                {here && (joining || reconnecting) && <Spinner size={13} />}

                {here && ! joining && ! reconnecting && (
                    <>
                        <button className="btn-icon size-[22px] rounded-[7px] border-brand/35 bg-brand/15 text-lilac-2" type="button" title="Mudar visual para focado" onClick={() => hub.setFocusedRoom(true)}>
                            <Icon name="focus" size={11} />
                        </button>
                        <span className="font-mono text-[9.5px] text-ink-dim"><Elapsed since={connectedAt} /></span>
                    </>
                )}

                {! here && (
                    <span className={`font-mono text-[9.5px] ${people.length ? 'text-ink-dim' : 'text-ink-faint'}`}>{people.length ? `${people.length}` : 'vazio'}</span>
                )}
            </div>

            {people.length > 0 && (
                <div className="mt-2 flex flex-col gap-1.5 pl-5">
                    {people.map(person => {
                        const me = person.user_id === user?.id;
                        const live = person.sources?.includes('screen') || (me && here && sharing);
                        const member = tree?.members.find(item => item.user_id === person.user_id);

                        return (
                            <div key={person.user_id} className="group relative flex items-center gap-2" onMouseEnter={() => live && setPreview(person.user_id)} onMouseLeave={() => setPreview(null)}>
                                <button
                                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
                                    type="button"
                                    title={member ? 'Ações do membro' : person.name}
                                    onClick={event => member && hub.openMemberMenu(member, event.clientX, event.clientY)}
                                >
                                    <Avatar name={person.name} size={22} mine={me} />
                                    <span className={`truncate text-[12.5px] ${me ? 'text-ink-body' : 'text-ink-icon'}`}>{person.name}</span>
                                    {person.muted && <span className="text-danger" title="Microfone mutado"><Icon name="micOff" size={12} /></span>}
                                    {person.sources?.includes('camera') && <span className="text-ink-dim" title="Câmera ligada"><Icon name="camera" size={12} /></span>}
                                </button>

                                {member && ! me && (
                                    <button className="btn-icon size-[22px] flex-none rounded-[7px] opacity-0 group-hover:opacity-100 focus-visible:opacity-100" type="button" title="Banir, expulsar, desconectar e mais" onClick={event => hub.openMemberMenu(member, event.clientX, event.clientY)}>
                                        <Icon name="dots" size={13} />
                                    </button>
                                )}

                                {live && (
                                    <button className="live-badge flex-none cursor-pointer hover:brightness-110" type="button" title="Assistir transmissão" onClick={() => watch(person)}>
                                        <span className="size-[5px] rounded-full bg-white" />
                                        AO VIVO
                                    </button>
                                )}

                                {preview === person.user_id && (
                                    <div className="popover absolute top-6 right-0 z-30 w-52 animate-rise border-danger/40 p-2.5">
                                        <div className="relative flex aspect-video items-center justify-center overflow-hidden rounded-lg bg-gradient-to-br from-brand-dark/35 to-black">
                                            <span className="absolute inset-0 animate-sweep bg-gradient-to-r from-transparent via-brand/20 to-transparent" />
                                            <span className="relative font-mono text-[9px] tracking-widest text-lilac-2 uppercase">tela de {person.name}</span>
                                        </div>
                                        <button className="btn-primary mt-2 w-full py-1.5 text-[11.5px]" type="button" onClick={() => watch(person)}>Assistir transmissão</button>
                                        <p className="mt-1.5 text-center font-mono text-[9px] text-ink-dim">{here ? 'abre aqui do lado' : 'entra na voz e abre ao lado'}</p>
                                    </div>
                                )}
                            </div>
                        );
                    })}
                </div>
            )}
        </div>
    );
}
