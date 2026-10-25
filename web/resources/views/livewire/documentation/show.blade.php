<div class="documentation">
    <div class="documentation-layout">
        <aside class="documentation-sidebar" aria-label="Navegação da documentação" data-testid="documentation-sidebar">
            <a href="{{ route('documentation.index') }}" class="documentation-sidebar-home">Documentação</a>

            <nav>
                @foreach ($sections as $sectionSlug => $section)
                    <section class="documentation-sidebar-section" aria-labelledby="documentation-section-{{ $sectionSlug }}">
                        <h2 id="documentation-section-{{ $sectionSlug }}">{{ $section['title'] }}</h2>

                        <ul>
                            @foreach ($section['pages'] as $entry)
                                <li>
                                    <a
                                        href="{{ route('documentation.show', ['section' => $sectionSlug, 'page' => $entry['slug']]) }}"
                                        @class(['is-current' => $currentPage['section'] === $sectionSlug && $currentPage['slug'] === $entry['slug']])
                                        @if ($currentPage['section'] === $sectionSlug && $currentPage['slug'] === $entry['slug']) aria-current="page" @endif
                                    >{{ $entry['title'] }}</a>
                                </li>
                            @endforeach
                        </ul>
                    </section>
                @endforeach
            </nav>
        </aside>

        <main class="documentation-main">
            <p class="documentation-breadcrumb">{{ $currentPage['section_title'] }}</p>

            <article class="documentation-prose">
                {!! $markdown !!}
            </article>

            <nav class="documentation-pagination" aria-label="Navegação entre páginas">
                @if ($previousPage)
                    <a href="{{ route('documentation.show', ['section' => $previousPage['section'], 'page' => $previousPage['slug']]) }}" class="documentation-pagination-link">
                        <span>Anterior</span>
                        <strong>← {{ $previousPage['title'] }}</strong>
                    </a>
                @else
                    <span></span>
                @endif

                @if ($nextPage)
                    <a href="{{ route('documentation.show', ['section' => $nextPage['section'], 'page' => $nextPage['slug']]) }}" class="documentation-pagination-link is-next">
                        <span>Próxima</span>
                        <strong>{{ $nextPage['title'] }} →</strong>
                    </a>
                @endif
            </nav>
        </main>
    </div>
</div>
