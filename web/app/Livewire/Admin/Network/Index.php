<?php

declare(strict_types=1);

namespace App\Livewire\Admin\Network;

use App\Services\System\NetworkTraffic;
use Illuminate\View\View;
use Livewire\Attributes\Title;
use Livewire\Component;

#[Title('Rede do servidor')]
final class Index extends Component
{
    public function render(NetworkTraffic $traffic): View
    {
        $interfaces = $traffic->rates();

        return view('livewire.admin.network.index', [
            'available' => $traffic->available(),
            'interfaces' => $interfaces,
            'downloadMbps' => round(array_sum(array_column($interfaces, 'receivedMbps')), 2),
            'uploadMbps' => round(array_sum(array_column($interfaces, 'sentMbps')), 2),
        ]);
    }
}
