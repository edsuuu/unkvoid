/**
 * Recusa um instalador assinado por um par de chaves que o app não reconhece.
 *
 * A ferramenta de assinatura não sabe qual pública o aplicativo distribui. A recusa acontece
 * bem longe dali:
 * na máquina de quem instalou, na hora de atualizar, onde ninguém está olhando. Foi
 * assim que a 0.0.7 do Windows foi publicada e ninguém nunca conseguiu se atualizar.
 *
 *   node check-signature.mjs ../target/release/bundle/windows/Unkvoid_0.1.17_x64-setup.exe.sig
 *
 * Sai 1 quando alguma assinatura é de outra chave, ou quando não recebe arquivo nenhum —
 * build sem `.sig` também não atualiza ninguém.
 */
import { readFileSync } from 'node:fs';

/** Os dois bytes de algoritmo e os oito de identificador que abrem a chave e a assinatura. */
const keyId = (encoded) => {
    const text = Buffer.from(encoded.trim(), 'base64').toString('utf8');
    const body = text.split('\n').filter(line => line.trim() && ! line.startsWith('untrusted comment:') && ! line.startsWith('trusted comment:'));

    return Buffer.from(body[0].trim(), 'base64').subarray(2, 10).toString('hex');
};

const files = process.argv.slice(2);

if (! files.length) {
    console.error('[ERRO] nenhum .sig para conferir — o build saiu sem assinatura e ninguém se atualiza para ele');
    process.exit(1);
}

const expected = keyId(readFileSync(new URL('./update-public-key.pub', import.meta.url), 'utf8'));
let broken = 0;

for (const file of files) {
    const signed = keyId(readFileSync(file, 'utf8'));

    if (signed === expected) {
        console.log(`[INFO] ${file}: assinado pela chave ${signed}`);

        continue;
    }

    console.error(`[ERRO] ${file}: assinado pela chave ${signed}, e o app só aceita a ${expected}`);
    broken += 1;
}

process.exit(broken === 0 ? 0 : 1);
