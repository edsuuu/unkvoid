<?php

declare(strict_types=1);

namespace App\Livewire\Documentation;

use Illuminate\Support\Facades\File;
use Illuminate\Support\HtmlString;
use Illuminate\Support\Str;
use Illuminate\View\View;
use Livewire\Component;

final class Show extends Component
{
    public string $section = '';

    public string $page = '';

    public function mount(?string $section = null, ?string $page = null): void
    {
        $entry = $this->entry($section, $page);

        abort_if(is_null($entry), 404);

        $this->section = $entry['section'];
        $this->page = $entry['slug'];
    }

    public function render(): View
    {
        $currentPage = $this->entry($this->section, $this->page);

        abort_if(is_null($currentPage), 404);

        $pages = $this->pages();
        $currentIndex = array_search("{$this->section}/{$this->page}", array_column($pages, 'key'), true);

        return view('livewire.documentation.show', [
            'sections' => $this->sections(),
            'currentPage' => $currentPage,
            'previousPage' => is_int($currentIndex) && $currentIndex > 0 ? $pages[$currentIndex - 1] : null,
            'nextPage' => is_int($currentIndex) && isset($pages[$currentIndex + 1]) ? $pages[$currentIndex + 1] : null,
            'markdown' => new HtmlString(Str::markdown(File::get(resource_path('docs/'.$currentPage['file'])), [
                'html_input' => 'strip',
                'allow_unsafe_links' => false,
            ])),
        ]);
    }

    /**
     * @return array<string, array{title: string, pages: array<int, array{slug: string, title: string, file: string}>}>
     */
    private function sections(): array
    {
        /** @var array<string, array{title: string, pages: array<int, array{slug: string, title: string, file: string}>}> $sections */
        $sections = config('documentation.sections', []);

        return $sections;
    }

    /**
     * @return array{section: string, section_title: string, slug: string, title: string, file: string}|null
     */
    private function entry(?string $section, ?string $page): ?array
    {
        $sections = $this->sections();

        if (is_null($section) && is_null($page)) {
            $section = array_key_first($sections);
            $page = is_null($section) ? null : ($sections[$section]['pages'][0]['slug'] ?? null);
        }

        if (is_null($section) || is_null($page) || ! isset($sections[$section])) {
            return null;
        }

        foreach ($sections[$section]['pages'] as $entry) {
            if ($entry['slug'] !== $page) {
                continue;
            }

            return [...$entry, 'section' => $section, 'section_title' => $sections[$section]['title']];
        }

        return null;
    }

    /**
     * @return array<int, array{key: string, section: string, section_title: string, slug: string, title: string, file: string}>
     */
    private function pages(): array
    {
        $pages = [];

        foreach ($this->sections() as $section => $data) {
            foreach ($data['pages'] as $page) {
                $pages[] = [...$page, 'key' => "{$section}/{$page['slug']}", 'section' => $section, 'section_title' => $data['title']];
            }
        }

        return $pages;
    }
}
