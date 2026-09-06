const prefersDark = window.matchMedia('(prefers-color-scheme: dark)');

const apply = (appearance) => {
    localStorage.setItem('appearance', appearance);

    document.documentElement.classList.toggle(
        'dark',
        appearance === 'dark' || (appearance === 'system' && prefersDark.matches),
    );
};

window.appearance = {
    get current() {
        return localStorage.getItem('appearance') ?? 'system';
    },
    set(appearance) {
        apply(appearance);
    },
};

prefersDark.addEventListener('change', () => {
    if (window.appearance.current === 'system') {
        apply('system');
    }
});

import { VoiceStage } from './voice/VoiceStage.js';

new VoiceStage().start();
