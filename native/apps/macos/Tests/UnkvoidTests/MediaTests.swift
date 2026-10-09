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
    func pastTheCeilingTheQueueIsTrimmedBackToTheCushionInSmallSteps() {
        var lane = SoundLane(queued: SoundLane.longest + 1)

        #expect(lane.skip(for: 960) == SoundLane.trimStep)
        #expect(lane.trimming)

        lane.queued = SoundLane.cushion + 1
        #expect(lane.skip(for: 960) == SoundLane.trimStep, "ainda acima da folga: continua aparando")

        lane.queued = SoundLane.cushion
        #expect(lane.skip(for: 960) == 0, "na folga: para de aparar")
        #expect(!lane.trimming)

        // Um bloco menor que o passo é aparado inteiro, e não além dele.
        lane = SoundLane(queued: SoundLane.longest + 1)
        #expect(lane.skip(for: 100) == 100)
    }
}
