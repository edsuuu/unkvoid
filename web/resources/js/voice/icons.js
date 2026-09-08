/**
 * Ícones de estado, compartilhados entre a web e o app.
 *
 * Moram aqui porque os dois desenham a mesma lista de quem está no canal de voz, e
 * duplicar o path fazia o mudo do app não ser o mudo da web. Um só, os dois iguais.
 */
export const MIC_ON = '<path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 1 0-6 0v5a3 3 0 0 0 3 3z"/><path d="M18 11a1 1 0 1 0-2 0 4 4 0 0 1-8 0 1 1 0 1 0-2 0 6 6 0 0 0 5 5.917V19H9a1 1 0 1 0 0 2h6a1 1 0 1 0 0-2h-2v-2.083A6 6 0 0 0 18 11z"/>';

export const MIC_OFF = '<path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-5.4-1.8l4.2 4.2V11a1 1 0 0 1-1.8.6L12 14zM4.7 3.3a1 1 0 0 0-1.4 1.4l16 16a1 1 0 0 0 1.4-1.4l-3.2-3.2A6 6 0 0 0 18 11a1 1 0 1 0-2 0c0 .7-.18 1.35-.5 1.92l-1.5-1.5V11l-.02.02L9 6.05V6a3 3 0 0 1 .1-.75L4.7 3.3zM6 10a1 1 0 0 0-2 0 6 6 0 0 0 5 5.92V19H9a1 1 0 1 0 0 2h6a1 1 0 0 0 .7-1.71L13 16.58V17h-1a4 4 0 0 1-4-4v-1.17L6.4 10.24A1 1 0 0 0 6 10z"/>';

export const HEAD_ON = '<path d="M12 3a9 9 0 0 0-9 9v5a3 3 0 0 0 3 3h1a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H5v-.5a7 7 0 1 1 14 0v.5h-2a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h1a3 3 0 0 0 3-3v-5a9 9 0 0 0-9-9z"/>';

export const HEAD_OFF = '<path d="M4.7 3.3a1 1 0 0 0-1.4 1.4l3 3A8.96 8.96 0 0 0 3 12v5a3 3 0 0 0 3 3h1a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H5v-.5c0-1.3.36-2.5 1-3.53l12.3 12.32a1 1 0 0 0 1.4-1.42L4.7 3.3zM21 12a9 9 0 0 0-13.6-7.75l1.47 1.47A7 7 0 0 1 19 11.5v.5h-2a1 1 0 0 0-1 1v3.17l2 2A3 3 0 0 0 21 17v-5z"/>';

/** Marcador vermelho ao lado do nome de quem está com microfone ou áudio mudo. */
export const stateBadges = ({ muted, deafened }) => [
    muted ? MIC_OFF : null,
    deafened ? HEAD_OFF : null,
]
    .filter(Boolean)
    .map(icon => `<span class="flex shrink-0 items-center text-[#f23f43]"><svg class="size-4" fill="currentColor" viewBox="0 0 24 24">${icon}</svg></span>`)
    .join('');
