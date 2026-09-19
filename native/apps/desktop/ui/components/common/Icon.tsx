const SLASH = <path d="M4 4l16 16" />;

const PATHS = {
    arrowLeft: <path d="M19 12H5M11 6l-6 6 6 6" />,
    camera: <><rect x="3" y="6" width="13" height="12" rx="2.5" /><path d="M16 10.5l5-3v9l-5-3z" /></>,
    cameraOff: <><rect x="3" y="6" width="13" height="12" rx="2.5" /><path d="M16 10.5l5-3v9l-5-3z" />{SLASH}</>,
    chat: <path d="M4 5h16v11H9l-4 4v-4H4z" />,
    check: <path d="M5 12l5 5L19 7" />,
    chevronDown: <path d="M7 10l5 5 5-5" />,
    close: <path d="M6 6l12 12M18 6L6 18" />,
    copy: <><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3" /></>,
    crown: <path d="M4 18h16M4 8l4 4 4-7 4 7 4-4-2 10H6z" />,
    dots: <><circle cx="6" cy="12" r="1.3" fill="currentColor" /><circle cx="12" cy="12" r="1.3" fill="currentColor" /><circle cx="18" cy="12" r="1.3" fill="currentColor" /></>,
    download: <path d="M12 4v11M7 10l5 5 5-5M5 20h14" />,
    edit: <path d="M16.5 3.5l4 4L8 20H4v-4z" />,
    eye: <><path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" /></>,
    focus: <><rect x="3" y="5" width="18" height="14" rx="2" /><rect x="9.5" y="9.5" width="5" height="5" fill="currentColor" stroke="none" /></>,
    fullscreen: <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />,
    fullscreenExit: <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" />,
    gear: <><path d="M10.3 4.3a1 1 0 0 1 1-.8h1.4a1 1 0 0 1 1 .8l.3 1.6a7 7 0 0 1 1.7 1l1.5-.6a1 1 0 0 1 1.2.4l.7 1.2a1 1 0 0 1-.2 1.3l-1.2 1a7 7 0 0 1 0 2l1.2 1a1 1 0 0 1 .2 1.3l-.7 1.2a1 1 0 0 1-1.2.4l-1.5-.6a7 7 0 0 1-1.7 1l-.3 1.6a1 1 0 0 1-1 .8h-1.4a1 1 0 0 1-1-.8l-.3-1.6a7 7 0 0 1-1.7-1l-1.5.6a1 1 0 0 1-1.2-.4l-.7-1.2a1 1 0 0 1 .2-1.3l1.2-1a7 7 0 0 1 0-2l-1.2-1a1 1 0 0 1-.2-1.3l.7-1.2a1 1 0 0 1 1.2-.4l1.5.6a7 7 0 0 1 1.7-1z" /><circle cx="12" cy="12" r="2.5" /></>,
    grid: <><rect x="4" y="4" width="7" height="7" rx="1.2" /><rect x="13" y="4" width="7" height="7" rx="1.2" /><rect x="4" y="13" width="7" height="7" rx="1.2" /><rect x="13" y="13" width="7" height="7" rx="1.2" /></>,
    hash: <path d="M9 4L7 20M17 4l-2 16M4 9h16M3 15h16" />,
    headphones: <><path d="M4 15v-3a8 8 0 0 1 16 0v3" /><rect x="3" y="14" width="4" height="6" rx="1.5" /><rect x="17" y="14" width="4" height="6" rx="1.5" /></>,
    headphonesOff: <><path d="M4 15v-3a8 8 0 0 1 16 0v3" /><rect x="3" y="14" width="4" height="6" rx="1.5" /><rect x="17" y="14" width="4" height="6" rx="1.5" />{SLASH}</>,
    home: <path d="M4 11l8-7 8 7M6 10v10h12V10M10 20v-5h4v5" />,
    logout: <path d="M15 4h4v16h-4M10 8l-4 4 4 4M6 12h11" />,
    logs: <path d="M6 4h9l3 3v13H6zM9 10h6M9 14h6M9 18h4" />,
    menu: <path d="M5 7h14M5 12h14M5 17h14" />,
    mic: <><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0M12 18v3M8.5 21h7" /></>,
    micOff: <><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0M12 18v3M8.5 21h7" />{SLASH}</>,
    pause: <path d="M9 6v12M15 6v12" />,
    phoneOff: <path d="M6.6 10.8c1.2 2.4 3.2 4.4 5.6 5.6l2-2c.3-.3.7-.4 1-.2 1.1.4 2.3.6 3.5.6.6 0 1 .4 1 1V19c0 .6-.4 1-1 1-8.3 0-15-6.7-15-15 0-.6.4-1 1-1h3.2c.6 0 1 .4 1 1 0 1.2.2 2.4.6 3.5.1.4 0 .8-.3 1l-1.6 1.3z" fill="currentColor" stroke="none" transform="rotate(135 12 12)" />,
    play: <path d="M8 5v14l11-7z" fill="currentColor" />,
    plus: <path d="M12 5v14M5 12h14" />,
    refresh: <path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6" />,
    screen: <><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></>,
    signal: <path d="M5 19v-3M10 19v-7M15 19v-11M20 19V5" />,
    sliders: <><path d="M4 6h16M4 12h16M4 18h16" /><circle cx="9" cy="6" r="2" fill="currentColor" /><circle cx="15" cy="12" r="2" fill="currentColor" /><circle cx="8" cy="18" r="2" fill="currentColor" /></>,
    speaker: <><path d="M4 9h4l5-4v14l-5-4H4z" /><path d="M17 9a4 4 0 0 1 0 6" /></>,
    speakerOff: <><path d="M4 9h4l5-4v14l-5-4H4z" /><path d="M17 9l5 6M22 9l-5 6" /></>,
    stop: <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none" />,
    trash: <path d="M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13" />,
    users: <><circle cx="9" cy="8" r="3.5" /><path d="M3 20a6 6 0 0 1 12 0M16 4.5a3.5 3.5 0 0 1 0 7M21 20a6 6 0 0 0-4-5.6" /></>,
};

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 16, className = '' }: { name: IconName; size?: number; className?: string }) {
    return (
        <svg className={className} width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            {PATHS[name]}
        </svg>
    );
}
