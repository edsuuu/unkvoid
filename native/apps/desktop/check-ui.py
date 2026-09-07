"""Confere que a UI do desktop não ficou apontando para nada.

Um `getElementById` que não acha nada devolve null em silêncio, e uma classe que não
existe no CSS não gera erro nenhum — os dois quebram a tela sem uma linha no console.
Depois de renomear ids e classes nos três arquivos, isto é o que prova que bateu.
"""
import pathlib
import re
import sys

BASE = pathlib.Path(__file__).parent / 'ui'

html = (BASE / 'index.html').read_text()
css = (BASE / 'style.css').read_text()
js = '\n'.join((BASE / f).read_text() for f in ['api.js', 'app.js', 'p2p.js'])

ids_html = set(re.findall(r'id="([^"]+)"', html))
classes_html = {c for attr in re.findall(r'class="([^"]+)"', html) for c in attr.split()}
classes_css = set(re.findall(r'\.([a-zA-Z][\w-]*)', css))
dados_html = set(re.findall(r'\b(data-[a-z-]+)', html))

erros = []

# el('x') e getElementById('x')
for nome in set(re.findall(r"\bel\(\s*'([^']+)'", js)) | set(re.findall(r"getElementById\(\s*'([^']+)'", js)):
    if nome not in ids_html:
        erros.append(f"o JS procura o id '{nome}', que não existe no HTML")

# querySelector('[data-x]') e element.dataset
for nome in set(re.findall(r"querySelector(?:All)?\(\s*'\[(data-[a-z-]+)", js)):
    if nome not in dados_html and nome not in set(re.findall(r'\b(data-[a-z-]+)', js)):
        erros.append(f"o JS procura '{nome}', que ninguém escreve")

# classes que o JS cria têm que existir no CSS
for atributo in re.findall(r"className\s*=\s*[`'\"]([^`'\"]+)", js):
    # Só o trecho literal antes da primeira interpolação: dentro de `${...}` é valor em
    # tempo de execução, não nome de classe.
    for classe in re.findall(r'[a-zA-Z][\w-]*', atributo.split('${')[0]):
        if classe not in classes_css:
            erros.append(f"o JS usa a classe '{classe}', que não existe no CSS")

# e as do HTML também
for classe in classes_html:
    if classe not in classes_css:
        erros.append(f"o HTML usa a classe '{classe}', que não existe no CSS")

# hífen em nome de variável não compila, e foi exatamente o que um id chamado
# 'server-rail' causou quando virou também o nome da variável que o guarda
for nome in re.findall(r'\b(?:const|let|var)\s+([\w-]+)', js):
    if '-' in nome:
        erros.append(f"'{nome}' tem hífen: serve como id, não como nome de variável")

# nada de português sobrando em id, classe ou identificador
sobra = sorted({p for p in re.findall(r'\b(?:tela|voz|canal|canais|servidor|botao|erro|senha|nome|estado|quadro|falha|medidor|palco|trilha|mudo|vazio|qualidade|compartilhar|parar|relogio|tentativa)\b', html + css)})

if sobra:
    erros.append(f"português sobrando em id/classe: {sobra}")

for erro in sorted(set(erros)):
    print('FALHA:', erro)

sys.exit(1 if erros else 0)
