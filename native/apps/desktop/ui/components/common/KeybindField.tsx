import { useState, type KeyboardEvent, type MouseEvent } from 'react';

import { Platform } from '../../core/Platform.ts';

const MODIFIERS = ['Control', 'Shift', 'Alt', 'Meta', 'CapsLock', 'Tab'];

const USABLE = /^(Key[A-Z]|Digit[0-9]|F[0-9]{1,2}|Numpad[A-Za-z0-9]+|Arrow(Up|Down|Left|Right)|Space|Backquote|Minus|Equal|Bracket(Left|Right)|Semicolon|Quote|Comma|Period|Slash|Backslash|Insert|Home|End|PageUp|PageDown)$/;

const MOUSE_BUTTONS: Record<number, string> = { 1: 'Mouse3', 3: 'Mouse4', 4: 'Mouse5' };

const LABELS: Record<string, string> = {
    CmdOrCtrl: navigator.platform.startsWith('Mac') ? '⌘' : 'Ctrl',
    Control: 'Ctrl',
    Alt: navigator.platform.startsWith('Mac') ? '⌥' : 'Alt',
    Shift: '⇧',
    Super: '⌘',
};

type KeybindFieldProps = {
    value: string;
    bare?: boolean;
    onChange: (accelerator: string) => void;
};

export function KeybindField({ value, bare = false, onChange }: KeybindFieldProps) {
    const [capturing, setCapturing] = useState(false);
    const [refused, setRefused] = useState('');
    const mouse = bare && Platform.isWindows();

    const label = value === ''
        ? 'sem tecla'
        : value.split('+').map(part => LABELS[part] ?? part.replace(/^(Key|Digit)/, '').replace(/^Mouse(\d)$/, 'Mouse $1')).join(' + ');

    const modifiersOf = (event: KeyboardEvent | MouseEvent): string[] => [
        ...(event.metaKey ? ['Super'] : []),
        ...(event.ctrlKey ? ['Control'] : []),
        ...(event.altKey ? ['Alt'] : []),
        ...(event.shiftKey ? ['Shift'] : []),
    ];

    const swallowSideButton = (event: MouseEvent<HTMLButtonElement>) => {
        if (mouse && MOUSE_BUTTONS[event.button]) {
            event.preventDefault();
        }
    };

    const captureMouse = (event: MouseEvent<HTMLButtonElement>) => {
        const button = MOUSE_BUTTONS[event.button];

        if (! mouse || ! button) {
            return;
        }

        event.preventDefault();

        if (! capturing) {
            return;
        }

        onChange([...modifiersOf(event), button].join('+'));
        setRefused('');
        setCapturing(false);
    };

    const capture = (event: KeyboardEvent<HTMLButtonElement>) => {
        event.preventDefault();

        if (event.key === 'Escape') {
            setRefused('');
            setCapturing(false);

            return;
        }

        if (event.key === 'Delete' || event.key === 'Backspace') {
            onChange('');
            setRefused('');
            setCapturing(false);

            return;
        }

        if (MODIFIERS.includes(event.key)) {
            return;
        }

        if (! USABLE.test(event.code)) {
            setRefused('essa tecla o sistema não aceita');

            return;
        }

        const parts = modifiersOf(event);

        if (parts.length === 0 && ! bare) {
            setRefused('junte Ctrl, Alt ou Shift — tecla solta roubaria a digitação do sistema inteiro');

            return;
        }

        parts.push(event.code);
        onChange(parts.join('+'));
        setRefused('');
        setCapturing(false);
    };

    return (
        <span className="flex flex-col items-end gap-0.5">
            <button
                className={`field cursor-pointer px-2.5 py-1.5 text-left font-mono text-[11.5px] ${capturing ? 'border-brand/60 text-ink-strong' : 'text-ink-icon'}`}
                type="button"
                onClick={() => { setRefused(''); setCapturing(true); }}
                onBlur={() => { setRefused(''); setCapturing(false); }}
                onKeyDown={capturing ? capture : undefined}
                onMouseDown={captureMouse}
                onMouseUp={swallowSideButton}
                onAuxClick={swallowSideButton}
            >
                {capturing ? (mouse ? 'aperte a tecla ou o botão do mouse…' : 'aperte a tecla…') : label}
            </button>
            {refused && <span className="text-right text-[10.5px] text-danger">{refused}</span>}
        </span>
    );
}
