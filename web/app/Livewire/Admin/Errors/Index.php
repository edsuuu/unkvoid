<?php

declare(strict_types=1);

namespace App\Livewire\Admin\Errors;

use App\Models\ErrorReport;
use Flux\Flux;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;
use Throwable;

#[Title('Erros dos apps')]
final class Index extends Component
{
    public string $platform = '';

    /**
     * @throws Throwable
     */
    public function remove(int $reportId): void
    {
        $report = ErrorReport::query()->find($reportId);

        if (is_null($report)) {
            return;
        }

        $report->delete();

        Flux::toast(text: 'Erro apagado da lista.');
    }

    public function render(): View
    {
        $query = ErrorReport::query()->orderByDesc('last_seen_at')->orderByDesc('id');

        if (mb_trim($this->platform) !== '') {
            $query->where('platform', $this->platform);
        }

        $reports = [];

        foreach ($query->get() as $report) {
            $reports[] = [
                'id' => $report->id,
                'signature' => $report->signature,
                'version' => $report->version,
                'platform' => $report->platform,
                'occurrences' => $report->occurrences,
                'firstSeenAt' => $report->first_seen_at->setTimezone('America/Sao_Paulo')->format('d/m/Y H:i'),
                'lastSeenAt' => $report->last_seen_at->setTimezone('America/Sao_Paulo')->format('d/m/Y H:i'),
                'repeated' => $report->occurrences > 1,
                'log' => $report->log,
            ];
        }

        return view('livewire.admin.errors.index', [
            'reports' => $reports,
            'platforms' => ['windows' => 'Windows', 'macos' => 'macOS', 'linux' => 'Linux'],
            'endpoint' => route('api.errors.store'),
        ]);
    }
}
