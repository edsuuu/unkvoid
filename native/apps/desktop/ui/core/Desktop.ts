export class Desktop {
    static harden(): void {
        document.addEventListener('contextmenu', event => {
            if (! (event.target as HTMLElement | null)?.closest('input, textarea, [contenteditable="true"]')) {
                event.preventDefault();
            }
        });

        document.addEventListener('dragstart', event => event.preventDefault());

        document.addEventListener('keydown', event => {
            if (event.key === 'F5' || ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'r')) {
                event.preventDefault();
            }
        });

        document.addEventListener('wheel', event => {
            if (event.ctrlKey || event.metaKey) {
                event.preventDefault();
            }
        }, { passive: false });
    }
}
