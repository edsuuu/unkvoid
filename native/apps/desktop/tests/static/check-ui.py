"""Confere que a UI do desktop não ficou apontando para nada.

Uma classe de CSS que não existe não gera erro nenhum, e o vocabulário do desenho
(`style.css`, camada `components`) é escrito à mão: cada classe dele existe para matar uma
cadeia de utilitários repetida, e uma que ninguém usa é peso morto que envelhece sem ninguém
notar. O resto são as regras de organização dos componentes.

python3 tests/static/check-ui.py
"""
import pathlib
import re
import sys

# tests/static/check-ui.py -> dois níveis até native/apps/desktop.
BASE = pathlib.Path(__file__).resolve().parents[2] / 'ui'

css = (BASE / 'style.css').read_text()
typescript = sorted(path for pattern in ('*.ts', '*.tsx') for path in BASE.rglob(pattern))
sources = sorted([*typescript, *BASE.rglob('*.html')])
code = '\n'.join(path.read_text() for path in sources)

errors = []

if not typescript:
    errors.append('nenhum arquivo .ts/.tsx lido: o caminho da interface está errado')

written = set(re.findall(r'[\w-]+', code))

for name in sorted(set(re.findall(r'^    \.([a-z0-9-]+)', css, re.M))):
    if name not in written:
        errors.append(f"a classe '.{name}' do style.css não é usada por ninguém")

# Hífen em nome de variável não compila, e foi exatamente o que um id chamado 'server-rail'
# causou quando virou também o nome da variável que o guarda.
for name in re.findall(r'\b(?:const|let|var)\s+([\w-]+)', code):
    if '-' in name:
        errors.append(f"'{name}' tem hífen: serve como id, não como nome de variável")

# Português em id, classe ou atributo — e SÓ neles. O texto que o usuário lê é em português
# de propósito; o que não pode misturar idioma é o nome das coisas.
names = ' '.join(re.findall(r'\b(?:id|className|class|htmlFor|name|data-[a-z-]+)="([^"]*)"', code))
leftovers = sorted(set(re.findall(r'\b(?:tela|voz|canal|canais|servidor|botao|erro|senha|nome|estado|quadro|falha|palco|trilha|mudo|vazio|qualidade|compartilhar|parar|tentativa)\b', names)))

if leftovers:
    errors.append(f"português sobrando em id/classe: {leftovers}")

# Um componente por arquivo, com o nome do arquivo: é assim que se acha um componente pelo
# nome sem abrir pasta por pasta.
for path in sorted(BASE.rglob('*.tsx')):
    if path.name == 'main.tsx':
        continue

    exported = re.findall(r'^export (?:function|const|class) (\w+)', path.read_text(), re.M)

    if exported != [path.stem]:
        errors.append(f"{path.relative_to(BASE)} exporta {exported}: um componente por arquivo, com o nome do arquivo")

# O front em TypeScript não leva comentário: o dono decidiu que o nome explica, e o que ele não
# explica vira nome melhor. O texto entre aspas some antes (URL tem "//"), trocado por espaço
# para o número da linha continuar certo.
LITERALS = re.compile(r"'(?:\\.|[^'\\\n])*'|\"(?:\\.|[^\"\\\n])*\"|`(?:\\.|[^`\\])*`")

for path in typescript:
    blanked = LITERALS.sub(lambda match: re.sub(r'[^\n]', ' ', match.group()), path.read_text())

    for number, line in enumerate(blanked.splitlines(), 1):
        if '//' in line or '/*' in line:
            errors.append(f"{path.relative_to(BASE)}:{number} tem comentário: no front em TypeScript o nome explica")

for error in sorted(set(errors)):
    print('FALHA:', error)

if not errors:
    print('interface: ok — vocabulário de CSS usado, um componente por arquivo, nomes em inglês e nenhum comentário')

sys.exit(1 if errors else 0)
