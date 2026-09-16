export class Platform {
    static isLinux(): boolean {
        return /Linux/i.test(navigator.platform) || /Linux/i.test(navigator.userAgent);
    }

    static isWindows(): boolean {
        return /Win/i.test(navigator.platform) || /Windows/i.test(navigator.userAgent);
    }
}
