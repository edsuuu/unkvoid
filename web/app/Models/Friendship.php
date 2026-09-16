<?php

declare(strict_types=1);

namespace App\Models;

use App\Enums\FriendshipStatusEnum;
use App\Events\FriendshipUpdated;
use App\Exceptions\ForbiddenException;
use App\Models\Concerns\LogsFailedWrites;
use Carbon\CarbonImmutable;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Collection;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Validation\ValidationException;
use Override;
use OwenIt\Auditing\Auditable as AuditableTrait;
use OwenIt\Auditing\Contracts\Auditable;
use Throwable;

/**
 * @property int $id
 * @property int $requester_id
 * @property int $addressee_id
 * @property FriendshipStatusEnum $status
 * @property ?CarbonImmutable $responded_at
 * @property-read User $requester
 * @property-read User $addressee
 */
#[Fillable(['requester_id', 'addressee_id', 'status', 'responded_at'])]
final class Friendship extends Model implements Auditable
{
    use AuditableTrait;
    use LogsFailedWrites;

    /**
     * A linha entre duas pessoas, venha de qual lado vier.
     */
    public static function between(User $one, User $other): ?self
    {
        return self::query()
            ->where(fn ($query) => $query->where('requester_id', $one->id)->where('addressee_id', $other->id))
            ->orWhere(fn ($query) => $query->where('requester_id', $other->id)->where('addressee_id', $one->id))
            ->first();
    }

    /**
     * Tudo o que interessa a uma pessoa: os amigos, os pedidos que ela fez e os que
     * recebeu. Quem ela bloqueou vem junto para a interface saber esconder.
     *
     * @return Collection<int, self>
     */
    public static function listFor(User $user): Collection
    {
        return self::query()
            ->with(['requester', 'addressee'])
            ->where('requester_id', $user->id)
            ->orWhere('addressee_id', $user->id)
            ->latest('id')
            ->get();
    }

    /**
     * Pedir amizade. Pedir a quem já pediu para você aceita o pedido dele: é o que a
     * pessoa espera ao clicar, e evita duas linhas cruzadas para o mesmo par.
     *
     * @throws Throwable
     */
    public static function request(User $actor, User $target): self
    {
        if ($actor->id === $target->id) {
            throw ValidationException::withMessages(['user' => 'Não dá para adicionar você mesmo.']);
        }

        $existing = self::between($actor, $target);

        throw_if($existing?->status === FriendshipStatusEnum::Blocked, ForbiddenException::class, 'Esta pessoa não está aceitando pedidos.');

        if ($existing?->status === FriendshipStatusEnum::Accepted) {
            return $existing;
        }

        if ($existing?->addressee_id === $actor->id) {
            $existing->accept($actor);

            return $existing;
        }

        if ($existing instanceof self) {
            return $existing;
        }

        $friendship = self::write('falha ao pedir amizade', fn (): self => self::query()->create([
            'requester_id' => $actor->id,
            'addressee_id' => $target->id,
            'status' => FriendshipStatusEnum::Pending,
        ]), ['requester_id' => $actor->id, 'addressee_id' => $target->id]);

        self::broadcast(new FriendshipUpdated($friendship->load(['requester', 'addressee'])));

        return $friendship;
    }

    /**
     * Pede amizade pelo e-mail. Quem não existe e quem existe recebem a mesma resposta de
     * "não achei", para a busca não virar um jeito de descobrir quem tem conta aqui.
     *
     * @throws Throwable
     * @throws ValidationException
     */
    public static function requestByEmail(User $actor, string $email): self
    {
        $target = User::query()->where('email', mb_strtolower(mb_trim($email)))->first();

        if (is_null($target)) {
            throw ValidationException::withMessages(['email' => 'Ninguém com esse e-mail.']);
        }

        return self::request($actor, $target)->load(['requester', 'addressee']);
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function requester(): BelongsTo
    {
        return $this->belongsTo(User::class, 'requester_id');
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function addressee(): BelongsTo
    {
        return $this->belongsTo(User::class, 'addressee_id');
    }

    /**
     * @throws Throwable
     */
    public function accept(User $actor): void
    {
        throw_if($this->addressee_id !== $actor->id, ForbiddenException::class, 'Só quem recebeu o pedido pode aceitar.');

        // Quem foi bloqueado é sempre o `addressee` da linha: sem esta guarda, ele
        // "aceitaria" o próprio bloqueio e voltaria a mandar mensagem.
        throw_if($this->status === FriendshipStatusEnum::Blocked, ForbiddenException::class, 'Esta conversa está bloqueada.');

        self::write('falha ao aceitar a amizade', fn () => $this->update([
            'status' => FriendshipStatusEnum::Accepted,
            'responded_at' => now(),
        ]), ['friendship_id' => $this->id]);

        self::broadcast(new FriendshipUpdated($this->load(['requester', 'addressee'])));
    }

    /**
     * Recusar, desfazer a amizade e desbloquear são a mesma coisa: a linha some.
     *
     * @throws Throwable
     */
    public function remove(User $actor): void
    {
        $this->authorize($actor);

        // Desbloquear é apagar a linha, e só quem bloqueou pode: senão o bloqueado
        // desfazia o próprio bloqueio pela rota de recusar.
        throw_if(
            $this->status === FriendshipStatusEnum::Blocked && $this->requester_id !== $actor->id,
            ForbiddenException::class,
            'Só quem bloqueou pode desfazer.'
        );

        self::write('falha ao desfazer a amizade', fn () => $this->delete(), ['friendship_id' => $this->id]);

        self::broadcast(new FriendshipUpdated($this->load(['requester', 'addressee']), removed: true));
    }

    /**
     * @throws Throwable
     */
    public function block(User $actor): void
    {
        $this->authorize($actor);

        self::write('falha ao bloquear', fn () => $this->update([
            'requester_id' => $actor->id,
            'addressee_id' => $this->other($actor)->id,
            'status' => FriendshipStatusEnum::Blocked,
            'responded_at' => now(),
        ]), ['friendship_id' => $this->id]);

        self::broadcast(new FriendshipUpdated($this->load(['requester', 'addressee'])));
    }

    public function other(User $user): User
    {
        return $this->requester_id === $user->id ? $this->addressee : $this->requester;
    }

    /**
     * @return array<string, string>
     */
    #[Override]
    protected function casts(): array
    {
        return [
            'status' => FriendshipStatusEnum::class,
            'responded_at' => 'datetime',
        ];
    }

    private function authorize(User $actor): void
    {
        throw_if($this->requester_id !== $actor->id && $this->addressee_id !== $actor->id, ForbiddenException::class, 'Esta amizade não é sua.');
    }
}
