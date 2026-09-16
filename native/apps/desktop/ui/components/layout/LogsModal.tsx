import { useEffect, useRef, useState } from 'react';

import { Modal } from '../common/Modal.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function LogsModal() {
    const app = useApp();
    const { logsOpen, logsText, logPath } = useStore(app.store);
    const [copied, setCopied] = useState(false);
    const output = useRef<HTMLTextAreaElement>(null);

    useEffect(() => {
        if (output.current) {
            output.current.scrollTop = output.current.scrollHeight;
        }
    }, [logsText, logsOpen]);

    if (! logsOpen) {
        return null;
    }

    const copy = async () => {
        if (await app.copyLogs()) {
            setCopied(true);
            setTimeout(() => setCopied(false), 1200);
        }
    };

    return (
        <Modal
            title="Diagnóstico"
            subtitle={logPath ? `Arquivo completo, inclusive de execuções que travaram: ${logPath}` : 'Copie estes eventos depois de reproduzir o problema.'}
            width={820}
            onClose={() => app.closeLogs()}
            footer={(
                <>
                    <button className="btn-ghost" type="button" onClick={() => app.clearLogs()}>Limpar</button>
                    <span className="flex-1" />
                    <button className="btn-ghost" type="button" onClick={() => app.closeLogs()}>Fechar</button>
                    <button className="btn-primary px-4 py-2 text-[13px]" type="button" onClick={copy}>{copied ? 'Copiado!' : 'Copiar logs'}</button>
                </>
            )}
        >
            <textarea ref={output} className="field scroll-thin h-[55vh] w-full resize-none font-mono text-[11.5px] text-ink-icon" value={logsText} readOnly />
        </Modal>
    );
}
