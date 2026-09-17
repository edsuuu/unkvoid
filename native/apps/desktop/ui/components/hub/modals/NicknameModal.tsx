import { useId, useState } from 'react';

import { Modal } from '../../common/Modal.tsx';
import { Spinner } from '../../common/Spinner.tsx';
import { useApp } from '../../useApp.ts';
import { useStore } from '../../useStore.ts';

export function NicknameModal({ initial }: { initial: string }) {
    const hub = useApp().hub;
    const { nicknameError, nicknameBusy } = useStore(hub.store);
    const [name, setName] = useState(initial);
    const errorId = useId();

    return (
        <Modal
            title="Escolha seu apelido"
            subtitle="É como as pessoas te acham. Sem espaço, e ninguém mais pode usar o mesmo."
            width={440}
            footer={(
                <>
                    <button className="btn-quiet" type="button" onClick={() => void hub.logout()}>Sair da conta</button>
                    <span className="flex-1" />
                    <button className="btn-primary flex items-center gap-2 px-4 py-2 text-[13px]" type="submit" form="nickname-form" disabled={nicknameBusy}>
                        {nicknameBusy && <Spinner size={13} />}
                        Confirmar
                    </button>
                </>
            )}
        >
            <form id="nickname-form" noValidate onSubmit={event => { event.preventDefault(); void hub.confirmNickname(name); }}>
                <input
                    className="field w-full"
                    type="text"
                    maxLength={32}
                    value={name}
                    onChange={event => { setName(event.target.value.replace(/\s+/g, '')); hub.clearNicknameError(); }}
                    placeholder="edsu"
                    title="Letras, números, ponto e _ — sem espaço"
                    aria-label="Apelido"
                    aria-invalid={nicknameError !== ''}
                    aria-describedby={nicknameError !== '' ? errorId : undefined}
                    autoComplete="username"
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    autoFocus
                />
                {nicknameError !== '' && <span id={errorId} className="mt-1.5 block text-[11.5px] text-danger">{nicknameError}</span>}
            </form>
        </Modal>
    );
}
