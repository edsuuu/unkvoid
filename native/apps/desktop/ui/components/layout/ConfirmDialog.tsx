import { useEffect, useRef } from 'react';

import { Modal } from '../common/Modal.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function ConfirmDialog() {
    const app = useApp();
    const { dialog } = useStore(app.store);
    const cancelButton = useRef<HTMLButtonElement>(null);

    useEffect(() => {
        cancelButton.current?.focus();
    }, [dialog]);

    if (! dialog) {
        return null;
    }

    return (
        <Modal
            title="Tem certeza?"
            width={420}
            onClose={() => app.resolveDialog(false)}
            footer={(
                <>
                    <span className="flex-1" />
                    <button ref={cancelButton} className="btn-ghost" type="button" onClick={() => app.resolveDialog(false)}>Cancelar</button>
                    <button className="btn-danger font-semibold" type="button" onClick={() => app.resolveDialog(true)}>{dialog.confirmLabel}</button>
                </>
            )}
        >
            <p className="text-[13.5px] text-ink-body">{dialog.message}</p>
        </Modal>
    );
}
