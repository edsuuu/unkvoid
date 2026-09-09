/**
 * Publica uma release a partir dos instaladores que já estão nesta máquina.
 *
 * Existe porque o CI não pode gerar tudo: `.msi` só sai no Windows, `.dmg` só no macOS.
 * Quem tem a máquina roda isto, e a release vai ganhando plataforma sem que nenhuma
 * apague a outra — o `latest.json` publicado é lido de volta e mesclado, senão subir o
 * Windows depois do macOS deixaria todo Mac instalado sem para onde atualizar.
 *
 *   node release.mjs            publica os instaladores que encontrar
 *   node release.mjs --dry-run  mostra o que faria, sem tocar no GitHub
 *
 * Precisa do `gh` autenticado. Para o auto-update funcionar, o build tem que ter sido
 * feito com TAURI_SIGNING_PRIVATE_KEY definido — é ele que gera o `.sig` ao lado do
 * instalador. Sem `.sig` a release sai mesmo assim, só sem atualização automática.
 */
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const RAIZ = new URL('../../target/release/bundle/', import.meta.url).pathname;
const REPO = 'edsuuu/unkvoid';

// A chave que o app usa para se reconhecer. Tem que bater com o alvo em que o
// instalador foi gerado, senão o cliente baixa e recusa.
//
// O sufixo do instalador não é enfeite. O updater procura `{os}-{arch}-{instalador}` e
// só depois `{os}-{arch}`. Enquanto `deb` e `appimage` dividiam a chave `linux-x86_64`,
// o último do laço sobrescrevia o outro: quem instalou pelo .deb baixava o AppImage,
// o `is_deb` dos bytes dava falso e a atualização morria com `InvalidUpdaterFormat` —
// engolida num `console.warn`, numa janela que não tem console. O mesmo valia para
// msi contra nsis no Windows.
const PLATFORMS = {
    msi: 'windows-x86_64-msi',
    nsis: 'windows-x86_64-nsis',
    deb: 'linux-x86_64-deb',
    appimage: 'linux-x86_64-appimage',
    macos: 'darwin-aarch64',
};

/**
 * Quem responde quando o app não sabe dizer por qual instalador foi instalado.
 *
 * Os dois escolhidos são os que se viram sozinhos: o NSIS não precisa do Windows
 * Installer, e o AppImage não pede senha de root para se substituir.
 */
const FALLBACK = {
    'windows-x86_64-nsis': 'windows-x86_64',
    'linux-x86_64-appimage': 'linux-x86_64',
};

const dryRun = process.argv.includes('--dry-run');
const gh = (...args) => execFileSync('gh', args, { encoding: 'utf8' });

const { version } = JSON.parse(readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url)));
const tag = `v${version}`;

/** Instaladores desta máquina, com a assinatura ao lado quando ela existe. */
const found = Object.entries(PLATFORMS).flatMap(([pasta, alvo]) => {
    let files;

    try {
        files = readdirSync(join(RAIZ, pasta));
    } catch {
        return [];
    }

    return files
        // O `.tar.gz` do macOS é o que o updater baixa; o `.app` solto não serve.
        .filter(name => ! name.endsWith('.sig') && (pasta !== 'macos' || name.endsWith('.tar.gz')))
        .map(name => ({
            alvo,
            name,
            path: join(RAIZ, pasta, name),
            signature: files.includes(`${name}.sig`)
                ? readFileSync(join(RAIZ, pasta, `${name}.sig`), 'utf8').trim()
                : null,
        }));
});

if (! found.length) {
    console.error(`Nenhum instalador em ${RAIZ} — rode \`npx tauri build\` primeiro.`);
    process.exit(1);
}

for (const item of found) {
    const mb = (statSync(item.path).size / 1024 / 1024).toFixed(1);

    console.log(`${item.alvo.padEnd(24)} ${item.nome} (${mb} MB)${item.assinatura ? '' : '  SEM ASSINATURA'}`);
}

// O que já está publicado manda: uma plataforma que esta máquina não gera não pode
// sumir do manifesto só porque foi outra máquina que a subiu.
let manifest = { version, notes: `Unkvoid ${tag}`, pub_date: new Date().toISOString(), platforms: {} };

try {
    const published = gh('release', 'download', tag, '--repo', REPO, '--pattern', 'latest.json', '--output', '-');

    manifest.platforms = JSON.parse(published).platforms ?? {};
    console.log(`\nlatest.json publicado tem: ${Object.keys(manifesto.platforms).join(', ') || '(nada)'}`);
} catch {
    // Release ou manifesto ainda não existem: começa vazio mesmo.
}

const base = `https://github.com/${REPO}/releases/download/${tag}`;

for (const item of found.filter(item => item.signature)) {
    const entry = { signature: item.signature, url: `${base}/${item.nome}` };

    manifest.platforms[item.alvo] = entry;

    if (FALLBACK[item.alvo]) {
        manifest.platforms[FALLBACK[item.alvo]] = entry;
    }
}

if (! Object.keys(manifest.platforms).length) {
    console.warn('\nNenhuma assinatura: a release sai, mas ninguém se atualiza sozinho para ela.');
    console.warn('Para assinar, defina TAURI_SIGNING_PRIVATE_KEY e refaça o build.\n');
}

const files = found.map(item => item.path);

if (Object.keys(manifest.platforms).length) {
    writeFileSync('latest.json', `${JSON.stringify(manifesto, null, 2)}\n`);
    files.push('latest.json');
    console.log(`\nlatest.json com: ${Object.keys(manifesto.platforms).join(', ')}`);
}

if (dryRun) {
    console.log(`\n[dry-run] publicaria ${tag} com ${files.length} arquivo(s).`);
    process.exit(0);
}

// `--clobber` porque subir a segunda plataforma numa release que já existe é o caso
// normal aqui, não um erro.
const exists = (() => {
    try {
        gh('release', 'view', tag, '--repo', REPO);

        return true;
    } catch {
        return false;
    }
})();

if (exists) {
    gh('release', 'upload', tag, ...files, '--repo', REPO, '--clobber');
} else {
    // NÃO marcar como pré-lançamento: o app procura em /releases/latest/, e o "latest"
    // do GitHub ignora pré-lançamentos — foi o que fez a v0.2.0 até a v0.7.0 nunca
    // atualizarem ninguém.
    gh('release', 'create', tag, ...files, '--repo', REPO,
        '--title', `Unkvoid ${version}`, '--notes', `Unkvoid ${tag}`, '--latest');
}

console.log(`\nhttps://github.com/${REPO}/releases/tag/${tag}`);
