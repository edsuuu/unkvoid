"""Confere que o código está em inglês e os comentários em português.

O projeto mistura duas línguas de propósito: o **código** fala inglês, como as
bibliotecas em volta, e o **comentário** fala português, como quem o escreve. Sem esta
conferência a regra se perde em uma sessão — já se perdeu uma vez, e a única forma de
não perder de novo é executar este verificador.

A lista é de palavras que só existem em português. Nada de heurística: `total`, `item`
e `status` são iguais nas duas línguas e não podem entrar aqui.

python3 native/tests/static/check-language.py
"""
import pathlib
import re
import sys

# native/tests/static/check-language.py -> três níveis até a raiz do repositório.
# Errar isto faz o `rglob` não achar arquivo nenhum e o teste passar sempre, que é pior
# do que não existir.
REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[3]

PORTUGUESE_WORDS = {
    'aba', 'abas', 'achados', 'agora', 'alternar', 'alvo', 'alvos', 'amostra', 'amostras',
    'anterior', 'apresentacao', 'argumento', 'argumentos', 'arquivo', 'arquivos',
    'assinatura', 'atributos', 'ativa', 'atualizar', 'aviso', 'avanco', 'bandeiras',
    'bloco', 'bombear', 'botao', 'botoes', 'caminho', 'campos', 'carimbos', 'chamadas',
    'exclui', 'excluir',
    'chave', 'cliente', 'codigo', 'colunas', 'comando', 'comprimento', 'contador',
    'contar', 'dados', 'decorrido', 'descricao', 'destino', 'detalhe', 'dono', 'duracao',
    'entrada', 'entregue', 'erro', 'erros', 'escala', 'escolhido', 'escondido', 'espera',
    'evento', 'existe', 'existente', 'faixa', 'falha', 'falhas', 'faltando', 'ferramenta',
    'filtro', 'fim', 'formato', 'imagem', 'inicio', 'iniciar', 'itens', 'janela',
    'janelas', 'lista', 'manifesto', 'metodo', 'midia', 'molduras', 'motivo', 'mudo',
    'nome', 'nomes', 'nucleos', 'operacao', 'origem', 'ouvinte', 'pacote', 'pacotes',
    'parametros', 'parcial', 'pedidos', 'pendente', 'pessoa', 'pessoas', 'ponte',
    'posicao', 'prazo', 'prefixo', 'primeiro', 'primeira', 'problema', 'pronto',
    'prontos', 'propriedade', 'publicado', 'quadro', 'quadros', 'quantos', 'recolher',
    'referencias', 'reserva', 'resultado', 'saida', 'seco', 'servidor', 'silenciados',
    'sobra', 'tamanho', 'tela', 'telas', 'tentativa', 'tentativas', 'texto', 'tipo',
    'tipos', 'transmissao', 'trava', 'vazio', 'vivo',
}

# Onde procurar, e como reconhecer o começo de um comentário em cada linguagem.
SOURCES = [
    ('native/shared', '*.rs', '//'),
    # Durante a renomeação, os dois caminhos são aceitos para o app único continuar coberto.
    ('native/apps/unkvoid', '*.rs', '//'),
    ('native/release', '*.mjs', '//'),
    ('sfu/src', '*.ts', '//'),
]

DECLARATION = re.compile(
    r'\b(?:let|const|var|static|fn|struct|enum|function|class|type|interface|mut|readonly)\s+'
    r'(?:mut\s+)?([A-Za-z_$][\w$]*)'
)

def without_comment(line, in_block):
    """A linha sem a parte comentada, e se o bloco continua aberto."""
    if in_block:
        end = line.find('*/')

        return ('', True) if end == -1 else (line[end + 2:], False)
    start = line.find('/*')
    if start != -1 and '*/' not in line[start:]:
        return line[:start], True
    line = re.sub(r'/\*.*?\*/', '', line)
    return line.split('//')[0], False

def main():
    errors = []
    files_read = 0

    for directory, pattern, _ in SOURCES:
        for path in sorted((REPOSITORY_ROOT / directory).rglob(pattern)):
            if 'node_modules' in path.parts or 'target' in path.parts:
                continue

            in_block = False
            files_read += 1

            for number, line in enumerate(path.read_text(encoding='utf-8', errors='ignore').splitlines(), 1):
                code, in_block = without_comment(line, in_block)

                # Texto entre aspas é o que o usuário lê, e continua em português.
                code = re.sub(r'"[^"]*"|\'[^\']*\'|`[^`]*`', '""', code)

                for match in DECLARATION.finditer(code):
                    name = match.group(1)

                    if name.lower() in PORTUGUESE_WORDS:
                        target = path.relative_to(REPOSITORY_ROOT)
                        errors.append(f"{target}:{number} declara '{name}' — o código é em inglês")

    if not files_read:
        print('FALHA: nenhum arquivo foi lido — a raiz do repositório está errada')
        sys.exit(1)

    for error in sorted(set(errors)):
        print('FALHA:', error)

    if errors:
        print(f'\n{len(errors)} identificador(es) em português. Comentário pode; código não.')
        sys.exit(1)

    print('linguagem: ok — código em inglês, comentários em português')

main()
