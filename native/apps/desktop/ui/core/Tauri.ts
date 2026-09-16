export type TauriEvent<Payload> = { payload: Payload };

export type TauriBridge = {
    core: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> };
    event: { listen: (event: string, handler: (event: TauriEvent<never>) => void) => Promise<() => void> };
    window?: { getCurrentWindow?: () => { setFullscreen: (on: boolean) => Promise<void> } | undefined };
};

declare global {
    interface Window {
        __TAURI__?: TauriBridge;
    }
}

export class Tauri {
    static available(): boolean {
        return Boolean(window.__TAURI__?.core);
    }

    static invoke<Result = unknown>(command: string, args?: Record<string, unknown>): Promise<Result> {
        return window.__TAURI__!.core.invoke(command, args) as Promise<Result>;
    }

    static listen<Payload>(event: string, handler: (event: TauriEvent<Payload>) => void): Promise<() => void> {
        return window.__TAURI__!.event.listen(event, handler);
    }

    static setFullscreen(on: boolean): Promise<void> | undefined {
        return window.__TAURI__?.window?.getCurrentWindow?.()?.setFullscreen(on);
    }
}
