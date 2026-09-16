export function Spinner({ size = 16 }: { size?: number }) {
    return (
        <span
            className="inline-block flex-none animate-spin rounded-full border-2 border-white/15 border-t-brand"
            style={{ width: size, height: size }}
            role="status"
            aria-label="Carregando"
        />
    );
}
