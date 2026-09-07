/**
 * Keeps the open text channel in sync over Reverb.
 *
 * The subscription lives here and not in a Livewire Echo listener because Livewire fixes
 * its listeners at mount, and the channel is only known after someone clicks one — a
 * listener declared later would never subscribe.
 *
 * Only the id travels on the socket. The list is read by the component, with the same
 * permissions as any other render: a second place deciding who may read what is a second
 * place to get it wrong.
 */
export class ChatSocket {
    constructor() {
        this.channelId = null;
    }

    start() {
        window.addEventListener('text-channel-opened', event => this.watch(event.detail?.channelId ?? null));
    }

    watch(channelId) {
        if (this.channelId === channelId) {
            return;
        }

        // Leaving first matters: the socket must not outlive the permission that opened
        // it, and someone removed from a server would otherwise keep receiving.
        if (this.channelId) {
            window.Echo?.leave(`channel.${this.channelId}`);
        }

        this.channelId = channelId;

        if (! channelId || ! window.Echo) {
            return;
        }

        window.Echo.private(`channel.${channelId}`)
            .listen('MessageSent', () => window.Livewire?.dispatch('message-received'));
    }
}
