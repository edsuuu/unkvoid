import Foundation
import Testing

@testable import Unkvoid

/// A mídia que não precisa de aparelho para ser conferida: a regra da fila de som de cada
/// pessoa. O resto (motor que religa ao trocar de saída, câmera que religa, quadro-chave
/// pedido na hora) só se prova com fone, câmera e alguém transmitindo.
struct MediaTests {
    /// Sem passar do teto, nada é aparado — nem quando a fila está cheia até a folga.
    @Test
    func aQueueUnderTheCeilingIsLeftAlone() {
        var lane = SoundLane(queued: SoundLane.longest)

        #expect(lane.skip(for: 960) == 0)
        #expect(!lane.trimming)
    }

    /// Passou do teto, cada bloco perde 5 ms até a fila voltar à folga; daí para a frente,
    /// nada mais é aparado. Como no `sound.rs` do Windows: 200 ms de atraso somem em ~0,6 s.
    @Test
    func pastTheCeilingTheQueueIsTrimmedBackToTheTargetInSmallSteps() {
        var lane = SoundLane(queued: SoundLane.longest + 1)

        #expect(lane.skip(for: 960) == SoundLane.trimStep)
        #expect(lane.trimming)

        lane.queued = SoundLane.target + 1
        #expect(lane.skip(for: 960) == SoundLane.trimStep, "ainda acima do alvo: continua aparando")

        lane.queued = SoundLane.target
        #expect(lane.skip(for: 960) == 0, "no alvo: para de aparar")
        #expect(!lane.trimming)

        // Um bloco menor que o passo é aparado inteiro, e não além dele.
        lane = SoundLane(queued: SoundLane.longest + 1)
        #expect(lane.skip(for: 100) == 100)
    }

    /// Sair da voz com alguém mandando som: `forget(nil)` para cada tocador, e o `stop()` espera
    /// os callbacks dos blocos que descartou. Se um callback precisar do mesmo cadeado que o
    /// `forget` segura, a main fica presa para sempre — a bolinha girando ao sair de qualquer voz.
    @Test
    func leavingTheVoiceWithSoundQueuedComesBack() {
        let sound = Sound()
        let silence = Data(count: 960 * 2 * MemoryLayout<Float>.size)

        for _ in 0 ..< 10 {
            sound.play(silence, from: "mic:grace")
            sound.play(silence, from: "mic:linus")
        }

        let done = DispatchSemaphore(value: 0)

        Thread.detachNewThread {
            sound.forget(nil)
            done.signal()
        }

        #expect(done.wait(timeout: .now() + 2) == .success, "forget(nil) não voltou em 2 s: deadlock entre o stop() e o callback da fila")
    }

    /// O quadro-chave é pedido na primeira falha e de novo a cada segundo enquanto não chega;
    /// chegou, a próxima falha pede na hora.
    @Test
    func theKeyframeIsAskedAgainEverySecondUntilItArrives() {
        var asking = KeyframeAsking()
        let start = Date()

        let first = asking.shouldAsk(now: start)
        let tooSoon = asking.shouldAsk(now: start.addingTimeInterval(0.5))
        let aSecondLater = asking.shouldAsk(now: start.addingTimeInterval(1.1))

        #expect(first && !tooSoon && aSecondLater)

        asking.arrived()

        let afterArriving = asking.shouldAsk(now: start.addingTimeInterval(1.2))

        #expect(afterArriving)
    }
}
