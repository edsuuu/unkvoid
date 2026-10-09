import AVFoundation
import CoreAudio

/// O som dos outros, tocando; e o microfone, subindo.
///
/// O núcleo entrega PCM `Float` estéreo intercalado a 48 kHz e quer o microfone no mesmo
/// formato. Daqui para dentro é o `AVAudioEngine`: um tocador por transmissão, e a entrada
/// com o processamento de voz do sistema (cancelamento de eco e supressão de ruído).
final class Sound: @unchecked Sendable {
    private let output = AVAudioEngine()
    private let input = AVAudioEngine()
    private let gate = NSLock()

    /// Quem está falando, medido no som que chega de cada pessoa. Avisa só na virada, e a
    /// fala continua "acesa" por 350 ms depois do último pico: o Opus nem manda pacote no
    /// silêncio, então é o relógio, e não o próximo pacote, que apaga o anel.
    var onSpeaking: (@Sendable (String, Bool) -> Void)?
    private let levels = DispatchQueue(label: "unkvoid-speaking")
    private var lastLoud: [String: Date] = [:]
    private static let loudness: Float = 0.02
    private static let tail = 0.35
    private var players: [String: AVAudioPlayerNode] = [:]
    /// Quanto cada tocador ainda tem na fila, e se está aparando o atraso.
    private var lanes: [String: SoundLane] = [:]
    /// Escolhido antes de o primeiro bloco de som chegar: o tocador nasce já nesse volume.
    private var volumes: [String: Float] = [:]
    /// A saída escolhida, para apontar de novo quando o motor religa.
    private var speaker: AudioDeviceID?
    private var configurationWatcher: NSObjectProtocol?
    private var converter: AVAudioConverter?
    /// Só quem abriu o microfone mexe no `inputNode`: tocar nele já pede o aparelho ao
    /// sistema, e fazer isso à toa numa saída de sala custa segundos.
    private var listening = false

    private static let rate = 48_000.0

    private static let stereo = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 2)!

    /// O que o núcleo quer receber: `Float` intercalado.
    private static let wire = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: rate, channels: 2, interleaved: true)!

    init() {
        // O motor de saída para sozinho quando o aparelho muda de taxa ou de canais — os AirPods
        // ao abrir o microfone, uma troca de saída no meio da chamada. Os tocadores que já
        // existiam seguiriam empilhando blocos num motor parado: silêncio até alguém novo.
        configurationWatcher = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: output, queue: nil) { [weak self] _ in
            self?.restartOutput()
        }
    }

    deinit {
        configurationWatcher.map(NotificationCenter.default.removeObserver)
    }

    /// Um bloco de som de uma transmissão. Chamado da thread da mídia.
    func play(_ samples: Data, from producer: String) {
        let frames = samples.count / MemoryLayout<Float>.size / 2

        gate.lock()

        let skipped = lanes[producer, default: SoundLane()].skip(for: frames)

        gate.unlock()

        guard let (buffer, peak) = Self.buffer(from: samples, skipping: skipped) else {
            return
        }

        if peak > Self.loudness {
            heard(producer)
        }

        gate.lock()

        defer { gate.unlock() }

        guard let player = player(for: producer) else {
            return
        }

        let queued = Int(buffer.frameLength)

        lanes[producer]?.queued += queued
        player.scheduleBuffer(buffer, completionCallbackType: .dataConsumed) { [weak self] _ in
            guard let self else {
                return
            }

            self.gate.lock()
            self.lanes[producer]?.queued -= queued
            self.gate.unlock()
        }

        // Motor parado (troca de aparelho) ou tocador que nunca voltou: ninguém ouviria.
        if !output.isRunning {
            try? output.start()
        }

        if !player.isPlaying {
            player.play()
        }
    }

    /// Depois de o aparelho de saída mudar: o motor religa, cada tocador volta a tocar e a
    /// saída escolhida é apontada de novo. As filas zeram — o que estava nelas já se perdeu.
    private func restartOutput() {
        gate.lock()

        defer { gate.unlock() }

        guard !players.isEmpty else {
            return
        }

        Self.point(output.outputNode.audioUnit, to: speaker)

        do {
            try output.start()
        } catch {
            return
        }

        for (key, player) in players {
            lanes[key] = SoundLane()
            player.play()
        }
    }

    /// O volume de uma transmissão, de 0 a 1, só deste lado.
    func setVolume(_ volume: Float, of producer: String) {
        gate.lock()

        defer { gate.unlock() }

        volumes[producer] = volume
        players[producer]?.volume = volume
    }

    /// Quem parou de transmitir deixa de ter tocador. `nil` cala tudo, na saída da sala.
    func forget(_ producer: String?) {
        gate.lock()

        defer { gate.unlock() }

        for (key, player) in players where producer == nil || key == producer {
            player.stop()
            output.detach(player)
            players[key] = nil
            lanes[key] = nil
        }

        if players.isEmpty {
            output.stop()
        }
    }

    /// Um toque do app, já sintetizado, pela mesma saída das vozes.
    func chime(_ samples: Data) {
        guard let (buffer, _) = Self.buffer(from: samples) else {
            return
        }

        gate.lock()

        defer { gate.unlock() }

        player(for: Self.chimePlayer)?.scheduleBuffer(buffer, completionHandler: nil)
    }

    /// O PCM intercalado do núcleo vira o planar que o `AVAudioEngine` toca; o pico do canal
    /// esquerdo vem junto, para saber quem está falando sem varrer o bloco de novo. Os
    /// primeiros `skipping` quadros ficam de fora: é assim que a fila apara o atraso.
    private static func buffer(from samples: Data, skipping: Int = 0) -> (AVAudioPCMBuffer, Float)? {
        let total = samples.count / MemoryLayout<Float>.size / 2
        let frames = total - skipping

        guard frames > 0, let buffer = AVAudioPCMBuffer(pcmFormat: stereo, frameCapacity: AVAudioFrameCount(frames)), let channels = buffer.floatChannelData else {
            return nil
        }

        buffer.frameLength = AVAudioFrameCount(frames)

        var peak: Float = 0

        samples.withUnsafeBytes { raw in
            let interleaved = raw.bindMemory(to: Float.self)

            for frame in 0 ..< frames {
                let source = (frame + skipping) * 2

                channels[0][frame] = interleaved[source]
                channels[1][frame] = interleaved[source + 1]
                peak = max(peak, abs(interleaved[source]))
            }
        }

        return (buffer, peak)
    }

    private static let chimePlayer = "unkvoid:chime"

    /// A saída escolhida na barra de baixo. `nil` é a do sistema.
    func use(speaker: AudioDeviceID?) {
        gate.lock()

        defer { gate.unlock() }

        self.speaker = speaker
        Self.point(output.outputNode.audioUnit, to: speaker)
    }

    /// Abre o microfone e entrega cada bloco, já em 48 kHz estéreo intercalado, a `heard`.
    func listen(on microphone: AudioDeviceID?, cleaned: Bool, _ heard: @escaping @Sendable (UnsafeBufferPointer<Float>) -> Void) throws {
        mute()

        let node = input.inputNode

        Self.point(node.audioUnit, to: microphone)

        // O processamento de voz é o cancelamento de eco do sistema. Abaixar o som do jogo
        // quando a pessoa fala é o padrão dele, e aqui é exatamente o que ninguém quer.
        try? node.setVoiceProcessingEnabled(cleaned)
        node.voiceProcessingOtherAudioDuckingConfiguration = .init(enableAdvancedDucking: false, duckingLevel: .min)

        let format = node.outputFormat(forBus: 0)

        guard format.sampleRate > 0, let converter = AVAudioConverter(from: format, to: Self.wire) else {
            throw Failure.noMicrophone
        }

        self.converter = converter

        node.installTap(onBus: 0, bufferSize: 960, format: format) { [weak self] block, _ in
            self?.convert(block, heard)
        }

        input.prepare()

        try input.start()

        listening = true
    }

    func mute() {
        guard listening else {
            return
        }

        listening = false
        input.inputNode.removeTap(onBus: 0)
        input.stop()
        converter = nil
    }

    enum Failure: Error {
        case noMicrophone
    }

    private func convert(_ block: AVAudioPCMBuffer, _ heard: (UnsafeBufferPointer<Float>) -> Void) {
        guard let converter else {
            return
        }

        let capacity = AVAudioFrameCount(Double(block.frameLength) * Self.rate / block.format.sampleRate) + 32

        guard let converted = AVAudioPCMBuffer(pcmFormat: Self.wire, frameCapacity: capacity) else {
            return
        }

        var delivered = false
        var failure: NSError?

        converter.convert(to: converted, error: &failure) { _, status in
            if delivered {
                status.pointee = .noDataNow

                return nil
            }

            delivered = true
            status.pointee = .haveData

            return block
        }

        guard failure == nil, converted.frameLength > 0, let samples = converted.floatChannelData?[0] else {
            return
        }

        heard(UnsafeBufferPointer(start: samples, count: Int(converted.frameLength) * 2))
    }

    /// Chamar com o cadeado na mão.
    private func heard(_ producer: String) {
        levels.async {
            let wasQuiet = self.lastLoud[producer] == nil

            self.lastLoud[producer] = Date()

            if wasQuiet {
                self.onSpeaking?(producer, true)
            }

            self.levels.asyncAfter(deadline: .now() + Self.tail + 0.05) {
                guard let last = self.lastLoud[producer], Date().timeIntervalSince(last) >= Self.tail else {
                    return
                }

                self.lastLoud[producer] = nil
                self.onSpeaking?(producer, false)
            }
        }
    }

    private func player(for producer: String) -> AVAudioPlayerNode? {
        if let player = players[producer] {
            return player
        }

        let player = AVAudioPlayerNode()

        output.attach(player)
        output.connect(player, to: output.mainMixerNode, format: Self.stereo)

        if !output.isRunning {
            do {
                try output.start()
            } catch {
                output.detach(player)

                return nil
            }
        }

        player.volume = volumes[producer] ?? 1
        player.play()
        players[producer] = player

        return player
    }

    private static func point(_ unit: AudioUnit?, to device: AudioDeviceID?) {
        guard let unit, var device else {
            return
        }

        AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &device, UInt32(MemoryLayout<AudioDeviceID>.size))
    }
}

/// A fila de som de uma pessoa, em quadros (um quadro = os dois canais). A mesma regra do
/// `sound.rs` do Windows: passou do teto, o mais velho vai saindo aos poucos até sobrar a
/// folga — cortar tudo de uma vez comeria uma palavra inteira. Relógio de placa nunca bate
/// com o de quem manda, e sem teto o atraso só cresce; o microfone mutado manda silêncio, então
/// a fila nunca esvaziaria sozinha.
struct SoundLane {
    var queued = 0
    var trimming = false

    static let perMillisecond = 48
    /// Quanto se quer na fila em regime: o que uma rede aos trancos precisa.
    static let cushion = 40 * perMillisecond
    static let longest = 200 * perMillisecond
    static let trimStep = 5 * perMillisecond

    /// Quantos quadros do bloco que chegou devem ficar de fora.
    mutating func skip(for incoming: Int) -> Int {
        if queued > Self.longest {
            trimming = true
        }

        if trimming, queued <= Self.cushion {
            trimming = false
        }

        return trimming ? min(Self.trimStep, incoming) : 0
    }
}
