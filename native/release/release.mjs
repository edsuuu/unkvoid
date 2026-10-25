/**
 * Publica no GitHub os instaladores que já estão nesta máquina.
 *
 * O instalador do Windows só sai no Windows e o `.deb` só sai no Debian 12. O manifesto já
 * publicado é lido e mesclado para um artefato de outra máquina não desaparecer.
 *
 *   node native/release/release.mjs            publica os artefatos encontrados
 *   node native/release/release.mjs --dry-run  mostra o que faria, sem tocar no GitHub
 */
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, resolve } from 'node:path';

const nativeDirectory = resolve(fileURLToPath(new URL('.', import.meta.url)), '..');
const windowsBundleDirectory = join(nativeDirectory, 'target', 'release', 'bundle', 'windows');
const linuxPackageDirectory = join(nativeDirectory, 'target', 'deb12');
const repository = 'edsuuu/unkvoid';

const workspaceVersion = () => {
    let inWorkspacePackage = false;

    for (const line of readFileSync(join(nativeDirectory, 'Cargo.toml'), 'utf8').split(/\r?\n/)) {
        if (line === '[workspace.package]') {
            inWorkspacePackage = true;

            continue;
        }

        if (line.startsWith('[')) {
            inWorkspacePackage = false;
        }

        if (inWorkspacePackage) {
            const match = line.match(/^version\s*=\s*"([^"]+)"/);

            if (match) {
                return match[1];
            }
        }
    }

    throw new Error('workspace.package.version não encontrado em native/Cargo.toml');
};

const version = workspaceVersion();
const tag = `v${version}`;
const dryRun = process.argv.includes('--dry-run');
const gh = (...commandArguments) => execFileSync('gh', commandArguments, { encoding: 'utf8' });

const signed = (directory, name) => {
    try {
        return readFileSync(join(directory, `${name}.sig`), 'utf8').trim();
    } catch {
        return null;
    }
};

const windowsInstallers = () => {
    try {
        return readdirSync(windowsBundleDirectory)
            .filter(name => name.endsWith('-setup.exe'))
            .map(name => ({
                platform: 'windows-x86_64-nsis',
                name,
                path: join(windowsBundleDirectory, name),
                signature: signed(windowsBundleDirectory, name),
            }));
    } catch {
        return [];
    }
};

const linuxPackages = () => {
    try {
        return readdirSync(linuxPackageDirectory)
            .filter(name => name.endsWith('.deb'))
            .map(name => ({
                platform: 'linux-x86_64-deb',
                name,
                path: join(linuxPackageDirectory, name),
                signature: null,
            }));
    } catch {
        return [];
    }
};

// O site atualiza pelo instalador NSIS; o `.deb` fica no GitHub só para download manual.
const artifacts = [...windowsInstallers(), ...linuxPackages()];

if (! artifacts.length) {
    console.error(`Nenhum artefato em ${windowsBundleDirectory} ou ${linuxPackageDirectory} — gere o instalador ou o .deb primeiro.`);
    process.exit(1);
}

for (const artifact of artifacts) {
    const megabytes = (statSync(artifact.path).size / 1024 / 1024).toFixed(1);

    console.log(`${artifact.platform.padEnd(24)} ${artifact.name} (${megabytes} MB)${artifact.signature ? '' : '  SEM ASSINATURA'}`);
}

// O que já está publicado manda: um artefato que esta máquina não gera não pode sumir só
// porque a publicação seguinte foi feita em outro sistema.
const manifest = { version, notes: `Unkvoid ${tag}`, pub_date: new Date().toISOString(), platforms: {} };

try {
    const published = gh('release', 'download', tag, '--repo', repository, '--pattern', 'latest.json', '--output', '-');

    manifest.platforms = JSON.parse(published).platforms ?? {};
    console.log(`\nlatest.json publicado tem: ${Object.keys(manifest.platforms).join(', ') || '(nada)'}`);
} catch {
    // Release ou manifesto ainda não existem: começa vazio mesmo.
}

const releaseBaseUrl = `https://github.com/${repository}/releases/download/${tag}`;

for (const artifact of artifacts.filter(artifact => artifact.signature)) {
    manifest.platforms[artifact.platform] = {
        signature: artifact.signature,
        url: `${releaseBaseUrl}/${artifact.name}`,
    };
}

if (! Object.keys(manifest.platforms).length) {
    console.warn('\nNenhuma assinatura: a release sai, mas ninguém se atualiza sozinho para ela.');
}

const files = artifacts.map(artifact => artifact.path);

if (Object.keys(manifest.platforms).length) {
    writeFileSync('latest.json', `${JSON.stringify(manifest, null, 2)}\n`);
    files.push('latest.json');
    console.log(`\nlatest.json com: ${Object.keys(manifest.platforms).join(', ')}`);
}

if (dryRun) {
    console.log(`\n[dry-run] publicaria ${tag} com ${files.length} arquivo(s).`);
    process.exit(0);
}

const exists = (() => {
    try {
        gh('release', 'view', tag, '--repo', repository);

        return true;
    } catch {
        return false;
    }
})();

if (exists) {
    gh('release', 'upload', tag, ...files, '--repo', repository, '--clobber');
} else {
    // Não marcar como pré-lançamento: o cliente consulta a release mais recente do GitHub.
    gh('release', 'create', tag, ...files, '--repo', repository,
        '--title', `Unkvoid ${version}`, '--notes', `Unkvoid ${tag}`, '--latest');
}

console.log(`\nhttps://github.com/${repository}/releases/tag/${tag}`);
