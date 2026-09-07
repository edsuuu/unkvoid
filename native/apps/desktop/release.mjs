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
const PLATAFORMAS = {
    msi: 'windows-x86_64',
    nsis: 'windows-x86_64',
    deb: 'linux-x86_64',
    appimage: 'linux-x86_64',
    macos: 'darwin-aarch64',
};

const seco = process.argv.includes('--dry-run');
const gh = (...args) => execFileSync('gh', args, { encoding: 'utf8' });

const { version } = JSON.parse(readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url)));
const tag = `v${version}`;

/** Instaladores desta máquina, com a assinatura ao lado quando ela existe. */
const achados = Object.entries(PLATAFORMAS).flatMap(([pasta, alvo]) => {
    let arquivos;

    try {
        arquivos = readdirSync(join(RAIZ, pasta));
    } catch {
        return [];
    }

    return arquivos
        // O `.tar.gz` do macOS é o que o updater baixa; o `.app` solto não serve.
        .filter(nome => ! nome.endsWith('.sig') && (pasta !== 'macos' || nome.endsWith('.tar.gz')))
        .map(nome => ({
            alvo,
            nome,
            caminho: join(RAIZ, pasta, nome),
            assinatura: arquivos.includes(`${nome}.sig`)
                ? readFileSync(join(RAIZ, pasta, `${nome}.sig`), 'utf8').trim()
                : null,
        }));
});

if (! achados.length) {
    console.error(`Nenhum instalador em ${RAIZ} — rode \`npx tauri build\` primeiro.`);
    process.exit(1);
}

for (const item of achados) {
    const mb = (statSync(item.caminho).size / 1024 / 1024).toFixed(1);

    console.log(`${item.alvo.padEnd(15)} ${item.nome} (${mb} MB)${item.assinatura ? '' : '  SEM ASSINATURA'}`);
}

// O que já está publicado manda: uma plataforma que esta máquina não gera não pode
// sumir do manifesto só porque foi outra máquina que a subiu.
let manifesto = { version, notes: `Unkvoid ${tag}`, pub_date: new Date().toISOString(), platforms: {} };

try {
    const publicado = gh('release', 'download', tag, '--repo', REPO, '--pattern', 'latest.json', '--output', '-');

    manifesto.platforms = JSON.parse(publicado).platforms ?? {};
    console.log(`\nlatest.json publicado tem: ${Object.keys(manifesto.platforms).join(', ') || '(nada)'}`);
} catch {
    // Release ou manifesto ainda não existem: começa vazio mesmo.
}

const base = `https://github.com/${REPO}/releases/download/${tag}`;

for (const item of achados.filter(item => item.assinatura)) {
    manifesto.platforms[item.alvo] = { signature: item.assinatura, url: `${base}/${item.nome}` };
}

if (! Object.keys(manifesto.platforms).length) {
    console.warn('\nNenhuma assinatura: a release sai, mas ninguém se atualiza sozinho para ela.');
    console.warn('Para assinar, defina TAURI_SIGNING_PRIVATE_KEY e refaça o build.\n');
}

const arquivos = achados.map(item => item.caminho);

if (Object.keys(manifesto.platforms).length) {
    writeFileSync('latest.json', `${JSON.stringify(manifesto, null, 2)}\n`);
    arquivos.push('latest.json');
    console.log(`\nlatest.json com: ${Object.keys(manifesto.platforms).join(', ')}`);
}

if (seco) {
    console.log(`\n[dry-run] publicaria ${tag} com ${arquivos.length} arquivo(s).`);
    process.exit(0);
}

// `--clobber` porque subir a segunda plataforma numa release que já existe é o caso
// normal aqui, não um erro.
const existe = (() => {
    try {
        gh('release', 'view', tag, '--repo', REPO);

        return true;
    } catch {
        return false;
    }
})();

if (existe) {
    gh('release', 'upload', tag, ...arquivos, '--repo', REPO, '--clobber');
} else {
    // NÃO marcar como pré-lançamento: o app procura em /releases/latest/, e o "latest"
    // do GitHub ignora pré-lançamentos — foi o que fez a v0.2.0 até a v0.7.0 nunca
    // atualizarem ninguém.
    gh('release', 'create', tag, ...arquivos, '--repo', REPO,
        '--title', `Unkvoid ${version}`, '--notes', `Unkvoid ${tag}`, '--latest');
}

console.log(`\nhttps://github.com/${REPO}/releases/tag/${tag}`);
