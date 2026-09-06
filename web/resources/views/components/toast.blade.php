<div
    x-data="{
        toasts: [],
        push(detail) {
            const toast = { id: ++this.lastId, text: detail.text, variant: detail.variant ?? 'info' };

            this.toasts.push(toast);

            setTimeout(() => this.toasts = this.toasts.filter((item) => item.id !== toast.id), 4000);
        },
        lastId: 0,
    }"
    x-on:toast.window="push($event.detail)"
    class="pointer-events-none fixed end-4 top-4 z-50 flex flex-col gap-2"
>
    <template x-for="toast in toasts" :key="toast.id">
        <div
            x-transition
            role="status"
            class="pointer-events-auto rounded-lg px-4 py-3 text-sm font-medium text-white shadow-lg"
            :class="{
                'bg-green-600': toast.variant === 'success',
                'bg-red-600': toast.variant === 'danger',
                'bg-zinc-900 dark:bg-zinc-700': ! ['success', 'danger'].includes(toast.variant),
            }"
            x-text="toast.text"
        ></div>
    </template>
</div>
