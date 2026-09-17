import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { AppContext } from '../../ui/components/AppContext.ts';
import { HubModals } from '../../ui/components/hub/HubModals.tsx';
import type { App } from '../../ui/core/App.ts';
import type { User } from '../../ui/core/Models.ts';
import { Store } from '../../ui/core/Store.ts';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('modal do apelido: aparece só para quem não confirmou, e não fecha', () => {
    let container: HTMLDivElement;
    let root: Root;

    const render = (user: Partial<User> | null) => {
        const store = new Store({ user, modal: null, roleEditor: null, memberMenu: null, tree: null, nicknameError: '', nicknameBusy: false });
        const app = { hub: { store, confirmNickname: async () => {}, clearNicknameError() {}, logout: async () => {} } } as unknown as App;

        act(() => root.render(
            <AppContext.Provider value={app}>
                <HubModals />
            </AppContext.Provider>,
        ));
    };

    beforeEach(() => {
        container = document.createElement('div');
        document.body.append(container);
        root = createRoot(container);
    });

    afterEach(() => {
        act(() => root.unmount());
        container.remove();
    });

    it('conta nova, pelo e-mail ou pelo Google, vê o modal com o apelido automático e sem jeito de fechar', () => {
        render({ id: 1, name: 'edsu4821', nickname_confirmed: false });

        expect(container.querySelector('[role="dialog"]')?.getAttribute('aria-label')).toBe('Escolha seu apelido');
        expect(container.querySelector('input')?.value).toBe('edsu4821');
        expect(container.querySelector('button[title="Fechar"]'), 'sem X').toBeNull();
        expect([...container.querySelectorAll('button')].map(button => button.textContent?.trim())).toEqual(['Sair da conta', 'Confirmar']);
    });

    it('quem já confirmou, ou sem conta, não vê o modal', () => {
        render({ id: 1, name: 'edsu', nickname_confirmed: true });
        expect(container.querySelector('[role="dialog"]')).toBeNull();

        render(null);
        expect(container.querySelector('[role="dialog"]')).toBeNull();
    });
});
