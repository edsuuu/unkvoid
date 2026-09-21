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
    /// Escolhido antes de o primeiro bloco de som chegar: o tocador nasce já nesse volume.
    private var volumes: [String: Float] = [:]
    private var converter: AVAudioConverter?
    /// Só quem abriu o microfone mexe no `inputNode`: tocar nele já pede o aparelho ao
    /// sistema, e fazer isso à toa numa saída de sala custa segundos.
    private var listening = false

    private static let rate = 48_000.0

    private static let stereo = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 2)!

    /// O que o núcleo quer receber: `Float` intercalado.
    private static let wire = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: rate, channels: 2, interleaved: true)!

    /// Um bloco de som de uma transmissão. Chamado da thread da mídia.
    func play(_ samples: Data, from producer: String) {
        let frames = samples.count / MemoryLayout<Float>.size / 2

        guard frames > 0, let buffer = AVAudioPCMBuffer(pcmFormat: Self.stereo, frameCapacity: AVAudioFrameCount(frames)) else {
            return
        }

        buffer.frameLength = AVAudioFrameCount(frames)

        samples.withUnsafeBytes { raw in
            let interleaved = raw.bindMemory(to: Float.self)

            guard let channels = buffer.floatChannelData else {
                return
            }

            var peak: Float = 0

            for frame in 0 ..< frames {
                channels[0][frame] = interleaved[frame * 2]
                channels[1][frame] = interleaved[frame * 2 + 1]
                peak = max(peak, abs(interleaved[frame * 2]))
            }

            if peak > Self.loudness {
                heard(producer)
            }
        }

        gate.lock()

        defer { gate.unlock() }

        guard let player = player(for: producer) else {
            return
        }

        player.scheduleBuffer(buffer)
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
        }

        if players.isEmpty {
            output.stop()
        }
    }

    /// A saída escolhida na barra de baixo. `nil` é a do sistema.
    func use(speaker: AudioDeviceID?) {
        gate.lock()

        defer { gate.unlock() }

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
