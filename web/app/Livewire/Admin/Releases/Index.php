<?php

declare(strict_types=1);

namespace App\Livewire\Admin\Releases;

use App\Enums\ReleasePlatformEnum;
use App\Models\Release;
use Flux\Flux;
use Illuminate\Validation\Rule;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;
use Livewire\Features\SupportFileUploads\TemporaryUploadedFile;
use Livewire\WithFileUploads;
use Throwable;

#[Title('Versões')]
final class Index extends Component
{
    use WithFileUploads;

    public string $version = '';

    public string $platform = '';

    public ?TemporaryUploadedFile $file = null;

    public ?TemporaryUploadedFile $signatureFile = null;

    public string $notes = '';

    /**
     * @throws Throwable
     */
    public function publish(): void
    {
        $this->validate([
            'version' => ['required', 'string', 'regex:/^\d+\.\d+\.\d+$/'],
            'platform' => ['required', Rule::enum(ReleasePlatformEnum::class)],
            'file' => ['required', 'file', 'max:204800'],
            'signatureFile' => ['nullable', 'file', 'max:16'],
            'notes' => ['nullable', 'string', 'max:2000'],
        ]);

        if (is_null($this->file)) {
            return;
        }

        $signature = is_null($this->signatureFile) ? null : mb_trim((string) file_get_contents($this->signatureFile->getRealPath()));

        Release::publish(
            $this->version,
            ReleasePlatformEnum::from($this->platform),
            $this->file,
            $signature === '' ? null : $signature,
            mb_trim($this->notes) === '' ? null : mb_trim($this->notes),
        );

        $this->reset('platform', 'file', 'signatureFile', 'notes');

        Flux::toast(variant: 'success', text: 'Versão publicada no bucket.');
    }

    /**
     * @throws Throwable
     */
    public function remove(int $releaseId): void
    {
        $release = Release::query()->find($releaseId);

        if (is_null($release)) {
            return;
        }

        $release->remove();

        Flux::toast(text: 'Versão apagada.');
    }

    public function render(): View
    {
        $releases = [];

        foreach (Release::query()->orderByDesc('published_at')->orderByDesc('id')->get() as $release) {
            $releases[] = [
                'id' => $release->id,
                'version' => $release->version,
                'platform' => $release->platform->label(),
                'fileName' => $release->file_name,
                'size' => number_format($release->size / 1048576, 1, ',', '.').' MB',
                'signed' => ! is_null($release->signature),
                'publishedAt' => $release->published_at->setTimezone('America/Sao_Paulo')->format('d/m/Y H:i'),
                'url' => $release->downloadUrl(),
            ];
        }

        $platforms = [];

        foreach (ReleasePlatformEnum::cases() as $platform) {
            $platforms[$platform->value] = $platform->label();
        }

        return view('livewire.admin.releases.index', [
            'releases' => $releases,
            'platforms' => $platforms,
            'manifestUrl' => route('downloads.manifest'),
        ]);
    }
}
