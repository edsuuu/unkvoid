/**
 * Recusa um instalador assinado por um par de chaves que o app não reconhece.
 *
 * O Tauri só AVISA quando a chave privada não bate com a `pubkey` do tauri.conf.json —
 * uma linha amarela no meio de mil, e o build passa. A recusa acontece bem longe dali:
 * na máquina de quem instalou, na hora de atualizar, onde ninguém está olhando. Foi
 * assim que a 0.0.7 do Windows foi publicada e ninguém nunca conseguiu se atualizar.
 *
 *   node check-signature.mjs ../../target/release/bundle/msi/Unkvoid_0.0.23_x64.msi.sig
 *
 * Sai 1 quando alguma assinatura é de outra chave, ou quando não recebe arquivo nenhum —
 * build sem `.sig` também não atualiza ninguém.
 */
import { readFileSync } from 'node:fs';

/** Os dois bytes de algoritmo e os oito de identificador que abrem a chave e a assinatura. */
const keyId = (base64) => {
    const texto = Buffer.from(base64.trim(), 'base64').toString('utf8');
    const corpo = texto.split('\n').filter(linha => linha.trim() && ! linha.startsWith('untrusted comment:') && ! linha.startsWith('trusted comment:'));

    return Buffer.from(corpo[0].trim(), 'base64').subarray(2, 10).toString('hex');
};

const files = process.argv.slice(2);

if (! files.length) {
    console.error('[ERRO] nenhum .sig para conferir — o build saiu sem assinatura e ninguém se atualiza para ele');
    process.exit(1);
}

const conf = JSON.parse(readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const expected = keyId(conf.plugins.updater.pubkey);
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
