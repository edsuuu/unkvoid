import AVFoundation

/// Os sons curtos do app, os mesmos toques do React (`ui/core/Sounds.ts`): senoides de
/// poucos décimos, com ataque de 12 ms e cauda exponencial. Sintetizados aqui, e não os do
/// sistema, para o Mac soar igual ao Windows e ao Linux — e tocados pelo motor de saída da
/// sala, para saírem no fone escolhido, e não na saída padrão.
@MainActor
enum Sounds {
    /// O motor por onde tocar. Sem ele (sem núcleo) o app fica em silêncio.
    static var output: Sound?

    struct Step {
        var hertz: Double
        var startsAt: Double
        var seconds = 0.09
    }

    static let volume: Float = 0.07
    private static let rate = 48_000.0

    static func joined() {
        play([Step(hertz: 523, startsAt: 0), Step(hertz: 784, startsAt: 0.08)])
    }

    static func left() {
        play([Step(hertz: 659, startsAt: 0), Step(hertz: 440, startsAt: 0.08)])
    }

    static func streamStarted() {
        play([Step(hertz: 587, startsAt: 0), Step(hertz: 740, startsAt: 0.08), Step(hertz: 880, startsAt: 0.16)])
    }

    static func streamStopped() {
        play([Step(hertz: 880, startsAt: 0), Step(hertz: 740, startsAt: 0.08), Step(hertz: 587, startsAt: 0.16)])
    }

    static func message() {
        play([Step(hertz: 988, startsAt: 0, seconds: 0.06), Step(hertz: 1319, startsAt: 0.05, seconds: 0.1)])
    }

    private static func play(_ steps: [Step]) {
        guard let output, let buffer = render(steps) else {
            return
        }

        output.chime(buffer)
    }

    /// Cada passo é uma senoide com envelope: sobe linear em 12 ms e cai exponencial até o fim,
    /// como o `linearRampToValueAtTime` + `exponentialRampToValueAtTime` do React.
    static func render(_ steps: [Step]) -> AVAudioPCMBuffer? {
        let length = steps.map { $0.startsAt + $0.seconds + 0.02 }.max() ?? 0
        let frames = AVAudioFrameCount(length * rate)

        guard frames > 0, let format = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 2), let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames), let channels = buffer.floatChannelData else {
            return nil
        }

        buffer.frameLength = frames

        for frame in 0 ..< Int(frames) {
            let time = Double(frame) / rate
            var sample: Float = 0

            for step in steps where time >= step.startsAt && time < step.startsAt + step.seconds {
                let elapsed = time - step.startsAt
                let envelope = elapsed < 0.012
                    ? Float(elapsed / 0.012)
                    : Float(pow(0.0001 / Double(volume), (elapsed - 0.012) / (step.seconds - 0.012)))

                sample += Float(sin(2 * .pi * step.hertz * time)) * volume * envelope
            }

            channels[0][frame] = sample
            channels[1][frame] = sample
        }

        return buffer
    }
}
