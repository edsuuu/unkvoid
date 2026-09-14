<?php

declare(strict_types=1);

namespace App\Livewire\Admin\Audit;

use App\Models\ChannelAccess;
use App\Models\ChannelAudit;
use App\Models\Server;
use App\Models\User;
use Illuminate\Database\Eloquent\Collection;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Attributes\Url;
use Livewire\Component;
use OwenIt\Auditing\Models\Audit;

#[Title('Auditoria')]
final class Index extends Component
{
    private const string TIMEZONE = 'America/Sao_Paulo';

    private const int STEP = 200;

    #[Url]
    public string $tab = 'accesses';

    public string $serverId = '';

    public string $email = '';

    public string $from = '';

    public string $until = '';

    public int $limit = self::STEP;

    public function showTab(string $tab): void
    {
        $this->tab = in_array($tab, ['accesses', 'changes'], true) ? $tab : 'accesses';
    }

    public function loadMore(): void
    {
        $this->limit += self::STEP;
    }

    public function render(): View
    {
        $servers = [];

        foreach (Server::query()->orderBy('name')->get() as $server) {
            $servers[$server->id] = $server->name;
        }

        $isAccesses = $this->tab !== 'changes';
        $accesses = $isAccesses ? $this->accesses() : [];
        $changes = $isAccesses ? [] : $this->changes();

        return view('livewire.admin.audit.index', [
            'servers' => $servers,
            'isAccesses' => $isAccesses,
            'accesses' => $accesses,
            'changes' => $changes,
            'canLoadMore' => count($isAccesses ? $accesses : $changes) === $this->limit,
        ]);
    }

    /**
     * @return array<int, array<string, mixed>>
     */
    private function accesses(): array
    {
        $query = ChannelAccess::query()->with(['user', 'channel.server'])->latest('joined_at')->orderByDesc('id')->limit($this->limit);

        if ($this->serverId !== '') {
            $query->whereHas('channel', fn ($channel) => $channel->where('server_id', (int) $this->serverId));
        }

        if (mb_trim($this->email) !== '') {
            $query->whereHas('user', fn ($user) => $user->where('email', 'like', '%'.mb_trim($this->email).'%'));
        }

        if ($this->from !== '') {
            $query->where('joined_at', '>=', $this->from.' 00:00:00');
        }

        if ($this->until !== '') {
            $query->where('joined_at', '<=', $this->until.' 23:59:59');
        }

        $rows = [];

        foreach ($query->get() as $access) {
            $rows[] = [
                'id' => $access->id,
                'joinedAt' => $access->joined_at->setTimezone(self::TIMEZONE)->format('d/m/Y H:i:s'),
                'leftAt' => $access->left_at?->setTimezone(self::TIMEZONE)->format('d/m/Y H:i:s'),
                'open' => is_null($access->left_at),
                'user' => $access->user->name.' <'.$access->user->email.'>',
                'server' => $access->channel->server->name,
                'channel' => $access->channel->name,
                'ip' => $access->ip,
                'sfuIp' => $access->sfu_ip,
                'userAgent' => $access->user_agent,
            ];
        }

        return $rows;
    }

    /**
     * @return array<int, array<string, mixed>>
     */
    private function changes(): array
    {
        $query = Audit::query()->orderByDesc('id')->limit($this->limit);

        if (mb_trim($this->email) !== '') {
            $query->where('user_type', User::class)->whereIn('user_id', User::query()->where('email', 'like', '%'.mb_trim($this->email).'%')->select('id'));
        }

        if ($this->from !== '') {
            $query->where('created_at', '>=', $this->from.' 00:00:00');
        }

        if ($this->until !== '') {
            $query->where('created_at', '<=', $this->until.' 23:59:59');
        }

        $audits = $query->get();
        $channels = $this->channelChanges();
        $users = User::query()
            ->whereIn('id', $audits->pluck('user_id')->filter()->merge($channels->pluck('user_id')->filter()))
            ->pluck('name', 'id');
        $rows = [];

        // O prefixo separa as duas fontes: a chave do pacote e a do histórico do canal
        // colidiriam na mesma lista, e o blade usa isto como `wire:key`.
        foreach ($audits as $index => $audit) {
            $rows[] = [
                'at' => $audit->created_at,
                'id' => 'a'.$index,
                'when' => $audit->created_at?->setTimezone(self::TIMEZONE)->format('d/m/Y H:i:s'),
                'user' => $users[$audit->getAttribute('user_id')] ?? '—',
                'event' => $audit->event,
                'model' => class_basename((string) $audit->auditable_type).' #'.$audit->auditable_id,
                'before' => $this->summarize($audit->old_values),
                'after' => $this->summarize($audit->new_values),
                'ip' => $audit->getAttribute('ip_address'),
            ];
        }

        foreach ($channels as $index => $change) {
            $rows[] = [
                'at' => $change->created_at,
                'id' => 'c'.$index,
                'when' => $change->created_at->setTimezone(self::TIMEZONE)->format('d/m/Y H:i:s'),
                'user' => $users[$change->user_id] ?? '—',
                'event' => $change->event,
                'model' => 'Channel #'.$change->channel_id,
                'before' => $this->summarize($change->old_values),
                'after' => $this->summarize($change->new_values),
                'ip' => $change->ip_address,
            ];
        }

        usort($rows, fn (array $left, array $right): int => $right['at'] <=> $left['at']);

        return array_slice($rows, 0, $this->limit);
    }

    /**
     * O histórico do canal mora em tabela própria (id de canal é ULID), então a aba de
     * alterações junta as duas fontes e ordena pela hora.
     *
     * @return Collection<int, ChannelAudit>
     */
    private function channelChanges(): Collection
    {
        $query = ChannelAudit::query()->orderByDesc('id')->limit($this->limit);

        if (mb_trim($this->email) !== '') {
            $query->whereIn('user_id', User::query()->where('email', 'like', '%'.mb_trim($this->email).'%')->select('id'));
        }

        if ($this->from !== '') {
            $query->where('created_at', '>=', $this->from.' 00:00:00');
        }

        if ($this->until !== '') {
            $query->where('created_at', '<=', $this->until.' 23:59:59');
        }

        return $query->get();
    }

    /**
     * @param  ?array<string, mixed>  $values
     */
    private function summarize(?array $values): string
    {
        $parts = [];

        foreach ($values ?? [] as $key => $value) {
            $parts[] = $key.': '.(is_scalar($value) || is_null($value) ? var_export($value, true) : '…');
        }

        return implode(' · ', $parts);
    }
}
