import { useState } from 'react';

type AvatarProps = {
    name: string | null | undefined;
    url?: string | null;
    size?: number;
    mine?: boolean;
    status?: 'online' | 'offline' | null;
    square?: boolean;
};

export function Avatar({ name, url = null, size = 32, mine = false, status = null, square = false }: AvatarProps) {
    const [brokenUrl, setBrokenUrl] = useState<string | null>(null);
    const broken = url !== null && brokenUrl === url;
    const words = String(name ?? '?').trim().split(/\s+/).filter(Boolean);
    const initials = (words.length > 1 ? words[0][0] + words[1][0] : (words[0] ?? '?').slice(0, 2)).toUpperCase();
    const statusColor = status === 'online' ? 'bg-online' : 'bg-offline';
    const radius = square ? Math.round(size / 3) : '50%';

    return (
        <span className="relative inline-flex flex-none">
            {url && ! broken
                ? <img className="flex-none object-cover" src={url} alt="" style={{ width: size, height: size, borderRadius: radius }} onError={() => setBrokenUrl(url)} />
                : (
                    <span
                        className={`avatar ${mine ? '' : 'avatar-flat'}`}
                        style={{ width: size, height: size, fontSize: Math.max(9, Math.round(size * 0.33)), borderRadius: radius }}
                    >
                        {initials}
                    </span>
                )}
            {status && (
                <span className={`absolute -right-px -bottom-px rounded-full border-2 border-[#0f0c18] ${statusColor}`} style={{ width: Math.max(8, size / 3.2), height: Math.max(8, size / 3.2) }} />
            )}
        </span>
    );
}
