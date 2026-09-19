import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { AppContext } from '../../ui/components/AppContext.ts';
import { KeybindField } from '../../ui/components/common/KeybindField.tsx';
import { HubModals } from '../../ui/components/hub/HubModals.tsx';
import type { App } from '../../ui/core/App.ts';
import type { User } from '../../ui/core/Models.ts';
import { Store } from '../../ui/core/Store.ts';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('campo de tecla: a de falar aceita tecla solta, e no Windows o botão do mouse', () => {
    let container: HTMLDivElement;
    let root: Root;
    let chosen: string[];

    const render = (value: string, bare: boolean) => act(() => root.render(<KeybindField value={value} bare={bare} onChange={accelerator => chosen.push(accelerator)} />));
    const field = () => container.querySelector('button')!;
    const warning = () => container.querySelector('.text-danger')?.textContent ?? '';
    const fire = (event: Event) => {
        act(() => {
            field().dispatchEvent(event);
        });

        return event;
    };
    const mouseDown = (button: number, extra: MouseEventInit = {}) => fire(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button, ...extra }));
    const keyDown = (code: string, extra: KeyboardEventInit = {}) => fire(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, code, key: code.replace('Key', '').toLowerCase(), ...extra }));
    const startCapture = () => act(() => field().click());
    const onPlatform = (platform: string) => Object.defineProperty(navigator, 'platform', { value: platform, configurable: true });

    beforeEach(() => {
        chosen = [];
        container = document.createElement('div');
        document.body.append(container);
        root = createRoot(container);
        onPlatform('Win32');
    });

    afterEach(() => {
        act(() => root.unmount());
        container.remove();
    });

    it('no Windows os botões do meio e laterais viram Mouse3, Mouse4 e Mouse5; esquerdo e direito nunca', () => {
        render('', true);
        startCapture();
        expect(field().textContent).toBe('aperte a tecla ou o botão do mouse…');

        mouseDown(0);
        mouseDown(2);
        expect(chosen, 'esquerdo e direito não são tecla de falar').toEqual([]);

        const side = mouseDown(3);

        expect(chosen).toEqual(['Mouse4']);
        expect(side.defaultPrevented, 'o botão lateral não pode voltar a página do WebView2').toBe(true);

        startCapture();
        mouseDown(1);
        startCapture();
        mouseDown(4, { ctrlKey: true });
        expect(chosen).toEqual(['Mouse4', 'Mouse3', 'Control+Mouse5']);
    });

    it('fora do modo de captura o botão lateral não escolhe nada', () => {
        render('KeyV', true);
        mouseDown(3);

        expect(chosen).toEqual([]);
    });

    it('o rótulo mostra "Mouse 4", com o modificador na frente quando há', () => {
        render('Mouse4', true);
        expect(field().textContent).toBe('Mouse 4');

        render('Control+Mouse5', true);
        expect(field().textContent).toBe('Ctrl + Mouse 5');
    });

    it('mutar e ensurdecer não aceitam mouse e continuam exigindo Ctrl, Alt ou Shift; a de falar aceita tecla solta sem aviso', () => {
        render('', false);
        startCapture();
        expect(field().textContent).toBe('aperte a tecla…');
        mouseDown(3);
        keyDown('KeyM');
        expect(chosen).toEqual([]);
        expect(warning()).toMatch(/tecla solta roubaria a digitação/);

        keyDown('KeyM', { ctrlKey: true });
        expect(chosen).toEqual(['Control+KeyM']);

        render('', true);
        startCapture();
        keyDown('KeyV');
        expect(chosen).toEqual(['Control+KeyM', 'KeyV']);
        expect(warning()).toBe('');
    });

    it('fora do Windows o mouse não entra: o atalho global de lá não entende botão', () => {
        onPlatform('MacIntel');
        render('', true);
        startCapture();
        expect(field().textContent).toBe('aperte a tecla…');

        mouseDown(3);
        expect(chosen).toEqual([]);
    });
});

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
