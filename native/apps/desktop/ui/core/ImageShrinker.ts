type Decoded = CanvasImageSource & { width: number; height: number };

export class ImageShrinker {
    static readonly MAX_BYTES = 2 * 1024 * 1024;
    static readonly MAX_SIDE = 2560;
    static readonly ACCEPTED = ['image/jpeg', 'image/png', 'image/webp', 'image/gif'];
    static readonly STEPS = [
        { scale: 1, quality: 0.9 },
        { scale: 1, quality: 0.75 },
        { scale: 0.75, quality: 0.75 },
        { scale: 0.5, quality: 0.7 },
        { scale: 0.35, quality: 0.6 },
    ];

    static async fit(file: File): Promise<File> {
        if (! file.type.startsWith('image/')) {
            throw new Error('só dá para anexar imagem');
        }

        if (ImageShrinker.ACCEPTED.includes(file.type) && file.size <= ImageShrinker.MAX_BYTES) {
            return file;
        }

        if (file.type === 'image/gif') {
            throw new Error('GIF acima de 2 MB não vai: reduzir mataria a animação');
        }

        const image = await ImageShrinker.decode(file);
        const base = Math.min(1, ImageShrinker.MAX_SIDE / Math.max(image.width, image.height));
        let type = 'image/webp';

        for (const step of ImageShrinker.STEPS) {
            const width = Math.max(1, Math.round(image.width * base * step.scale));
            const height = Math.max(1, Math.round(image.height * base * step.scale));
            let blob = await ImageShrinker.encode(image, width, height, type, step.quality);

            if (type === 'image/webp' && blob.type !== type) {
                type = 'image/jpeg';
                blob = await ImageShrinker.encode(image, width, height, type, step.quality);
            }

            if (ImageShrinker.ACCEPTED.includes(blob.type) && blob.size <= ImageShrinker.MAX_BYTES) {
                const name = file.name.replace(/\.[^.]*$/, '') || 'imagem';

                return new File([blob], `${name}.${blob.type.slice('image/'.length)}`, { type: blob.type });
            }
        }

        throw new Error('não deu para reduzir essa imagem para menos de 2 MB');
    }

    static decode(file: Blob): Promise<Decoded> {
        const url = URL.createObjectURL(file);
        const image = new Image();

        return new Promise<Decoded>((resolve, reject) => {
            image.onload = () => resolve(image);
            image.onerror = () => reject(new Error('não deu para ler essa imagem'));
            image.src = url;
        }).finally(() => URL.revokeObjectURL(url));
    }

    static encode(image: Decoded, width: number, height: number, type: string, quality: number): Promise<Blob> {
        return new Promise<Blob>((resolve, reject) => {
            const canvas = document.createElement('canvas');

            canvas.width = width;
            canvas.height = height;

            const context = canvas.getContext('2d');

            if (! context) {
                reject(new Error('não deu para abrir o canvas'));

                return;
            }

            if (type === 'image/jpeg') {
                context.fillStyle = '#fff';
                context.fillRect(0, 0, width, height);
            }

            context.drawImage(image, 0, 0, width, height);
            canvas.toBlob(blob => (blob ? resolve(blob) : reject(new Error('não deu para exportar a imagem'))), type, quality);
        });
    }
}
