# Laravel Template Architecture

Especificação técnica das dependências e ferramentas integradas ao projeto.

## Stack Tecnológico

- **Runtime**: PHP 8.4+
- **Framework**: Laravel 13.x
- **Frontend Engine**: Livewire 4.x
- **UI Architecture**: Blade Components (Tailwind)
- **CSS Engine**: Tailwind CSS 4.x
- **Build Tool**: Vite 8.x

## Dependências de Desenvolvimento e QA

- **Análise Estática**: Larastan / PHPStan (Nível 9)
- **Code Style Fixer**: Laravel Pint
- **Refatoração Automática**: Rector
- **Testing Framework**: Pest PHP
- **Log Management**: `opcodesio/log-viewer`, `laravel/pail`

## Scripts de Automação (Composer)

| Comando | Descrição Técnica |
| :--- | :--- |
| `composer setup` | Orquestração de instalação: Dependências, Env, Keys, Migrations e Frontend Build. |
| `composer dev` | Execução paralela de processos: Server, Queue, Pail e Vite. |
| `composer lint` | Execução sequencial de Pint (format) e Rector (refactor). |
| `composer phpstan` | Análise estática de tipos via PHPStan. |
| `composer check` | Pipeline de validação: Static Analysis -> Lint Check -> Tests. |
| `composer test` | Execução de suíte de testes unitários e funcionais via Pest. |

## Instalação Rápida

Para iniciar um novo projeto utilizando este template, execute o comando abaixo:

```bash
curl -s https://raw.githubusercontent.com/edsuuu/template-laravel/main/create-project.sh | bash
```

> **Nota**: O instalador irá solicitar o nome do projeto e validar as dependências locais (PHP, Composer, PNPM/NPM).
