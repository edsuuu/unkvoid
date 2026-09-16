"""Confere que o código está em inglês e os comentários em português.

O projeto mistura duas línguas de propósito: o **código** fala inglês, como as
bibliotecas em volta, e o **comentário** fala português, como quem o escreve. Sem esta
conferência a regra se perde em uma sessão — já se perdeu uma vez, e a única forma de
não perder de novo é falhar o `npm run check`.

A lista é de palavras que só existem em português. Nada de heurística: `total`, `item`
e `status` são iguais nas duas línguas e não podem entrar aqui.

python3 tests/static/check-language.py
"""
import pathlib
import re
import sys

# native/apps/desktop/tests/static/check-language.py -> seis níveis até a raiz do repositório.
# Errar isto faz o `rglob` não achar arquivo nenhum e o teste passar sempre, que é pior
# do que não existir.
RAIZ = pathlib.Path(__file__).resolve().parents[5]

PORTUGUES = {
    'aba', 'abas', 'achados', 'agora', 'alternar', 'alvo', 'alvos', 'amostra', 'amostras',
    'anterior', 'apresentacao', 'argumento', 'argumentos', 'arquivo', 'arquivos',
    'assinatura', 'atributos', 'ativa', 'atualizar', 'aviso', 'avanco', 'bandeiras',
    'bloco', 'bombear', 'botao', 'botoes', 'caminho', 'campos', 'carimbos', 'chamadas',
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
FONTES = [
    ('native/crates', '*.rs', '//'),
    ('native/apps/desktop/src-tauri/src', '*.rs', '//'),
    ('native/apps/desktop/ui', '*.ts', '//'),
    ('native/apps/desktop/ui', '*.tsx', '//'),
    ('native/apps/desktop/tests', '*.ts', '//'),
    ('native/apps/desktop/tests', '*.tsx', '//'),
    ('sfu/src', '*.ts', '//'),
]

DECLARACAO = re.compile(
    r'\b(?:let|const|var|static|fn|struct|enum|function|class|type|interface|mut|readonly)\s+'
    r'(?:mut\s+)?([A-Za-z_$][\w$]*)'
)

def sem_comentario(linha, dentro):
    """A linha sem a parte comentada, e se o bloco continua aberto."""
    if dentro:
        fim = linha.find('*/')

        return ('', True) if fim == -1 else (linha[fim + 2:], False)
    inicio = linha.find('/*')
    if inicio != -1 and '*/' not in linha[inicio:]:
        return linha[:inicio], True
    linha = re.sub(r'/\*.*?\*/', '', linha)
    return linha.split('//')[0], False

def main():
    problemas = []
    lidos = 0

    for pasta, padrao, _ in FONTES:
        for caminho in sorted((RAIZ / pasta).rglob(padrao)):
            if 'node_modules' in caminho.parts or 'target' in caminho.parts:
                continue

            dentro = False
            lidos += 1

            for numero, linha in enumerate(caminho.read_text(errors='ignore').splitlines(), 1):
                codigo, dentro = sem_comentario(linha, dentro)

                # Texto entre aspas é o que o usuário lê, e continua em português.
                codigo = re.sub(r'"[^"]*"|\'[^\']*\'|`[^`]*`', '""', codigo)

                for encontrado in DECLARACAO.finditer(codigo):
                    nome = encontrado.group(1)

                    if nome.lower() in PORTUGUES:
                        alvo = caminho.relative_to(RAIZ)
                        problemas.append(f"{alvo}:{numero} declara '{nome}' — o código é em inglês")

    if not lidos:
        print('FALHA: nenhum arquivo foi lido — a raiz do repositório está errada')
        sys.exit(1)

    for problema in sorted(set(problemas)):
        print('FALHA:', problema)

    if problemas:
        print(f'\n{len(problemas)} identificador(es) em português. Comentário pode; código não.')
        sys.exit(1)

    print('linguagem: ok — código em inglês, comentários em português')

main()
