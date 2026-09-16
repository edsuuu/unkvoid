import { useEffect, useState } from 'react';

export function Elapsed({ since }: { since: number | null }) {
    const [now, setNow] = useState(Date.now);

    useEffect(() => {
        const timer = setInterval(() => setNow(Date.now()), 1000);

        return () => clearInterval(timer);
    }, []);

    if (! since) {
        return '0:00:00';
    }

    const seconds = Math.max(0, Math.floor((now - since) / 1000));
    const minutes = String(Math.floor(seconds / 60) % 60).padStart(2, '0');

    return `${Math.floor(seconds / 3600)}:${minutes}:${String(seconds % 60).padStart(2, '0')}`;
}
