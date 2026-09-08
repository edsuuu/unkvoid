<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Support\SfuToken;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

/**
 * Salas avulsas: sem conta, sem convite, sem banco.
 *
 * O código **é** a sala. Não há o que guardar: quem chega com um código entra numa sala
 * de mesmo nome no SFU, e quando o último sai ela deixa de existir. Persistir isso seria
 * criar uma tabela para guardar uma string que o próprio cliente já carrega.
 *
 * Anônimo de propósito — a pessoa baixa o app, cria uma sala e manda o código para o
 * amigo. O preço é que **o código é a única chave**: quem tem, entra e vê a tela. Por
 * isso ele é sorteado com entropia de verdade em vez de aceitar um nome escolhido à mão,
 * e a rota é limitada, senão varrer códigos seria só questão de tempo.
 */
final class RoomController extends Controller
{
    /** Seis bytes ≈ 2,8e14 combinações. Curto para colar no WhatsApp, longo para varrer. */
    private const CODE_BYTES = 6;

    public function __invoke(Request $request, SfuToken $sfuToken): JsonResponse
    {
        $data = $request->validate([
            'name' => ['required', 'string', 'min:1', 'max:40'],
            // Ausente significa "crie uma para mim". Só o formato que este controller
            // gera é aceito: nome escolhido à mão vira sala adivinhável.
            'room' => ['nullable', 'string', 'regex:/^[a-z0-9]{12}$/'],
        ]);

        $room = $data['room'] ?? bin2hex(random_bytes(self::CODE_BYTES));

        return response()->json([
            'room' => $room,
            'url' => config('services.sfu.url'),
            'token' => $sfuToken->issueGuest($room, $data['name']),
        ]);
    }
}
