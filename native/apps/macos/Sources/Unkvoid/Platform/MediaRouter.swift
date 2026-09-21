import Foundation

/// O que chega do núcleo para assistir, indo para quem desenha e para quem toca.
///
/// Uma thread só esvazia a fila (`unkvoid_next_media` é para uma thread só): quadro vai
/// para o `VideoSink` do producer, som vai para o `Sound`. Nada aqui passa pela main — 60
/// quadros por segundo na thread que desenha a interface a travariam.
final class MediaRouter: @unchecked Sendable {
    let sound = Sound()
    let camera = Camera()

    private let core: Core
    private let gate = NSLock()
    private var sinks: [String: VideoSink] = [:]
    /// O que chegou de uma transmissão antes de o cartão dela existir: do último keyframe
    /// em diante. Sem isto a imagem só abriria no keyframe seguinte.
    private var backlog: [String: [(frame: Data, keyframe: Bool)]] = [:]
    private var running = false

    /// Quanto se guarda à espera do cartão: dois segundos a 60 fps.
    private static let longestBacklog = 120

    init(core: Core) {
        self.core = core
    }

    /// O mesmo `VideoSink` para o cartão que desenha e para a thread que recebe. Nasce na
    /// main, que é onde o layer pode nascer, e já recebe o que chegou antes dele.
    @MainActor
    func sink(for producer: String) -> VideoSink {
        gate.lock()

        defer { gate.unlock() }

        if let sink = sinks[producer] {
            return sink
        }

        let sink = VideoSink()

        for waiting in backlog.removeValue(forKey: producer) ?? [] {
            sink.show(waiting.frame, keyframe: waiting.keyframe)
        }

        sinks[producer] = sink

        return sink
    }

    private func show(_ frame: Data, keyframe: Bool, from producer: String) {
        gate.lock()

        guard let sink = sinks[producer] else {
            if keyframe {
                backlog[producer] = [(frame, true)]
            } else if let waiting = backlog[producer], waiting.count < Self.longestBacklog {
                backlog[producer]?.append((frame, false))
            }

            gate.unlock()

            return
        }

        gate.unlock()

        sink.show(frame, keyframe: keyframe)
    }

    /// Fica só o que ainda está sendo assistido: layer de quem parou é memória de vídeo presa.
    func keep(_ producers: Set<String>) {
        gate.lock()
        sinks = sinks.filter { producers.contains($0.key) }
        backlog = backlog.filter { producers.contains($0.key) }
        gate.unlock()
    }

    func start() {
        gate.lock()

        defer { gate.unlock() }

        guard !running else {
            return
        }

        running = true

        let thread = Thread { [weak self] in
            while let self, self.isRunning {
                guard let media = self.core.nextMedia() else {
                    continue
                }

                switch media.kind {
                case let .video(keyframe, _): self.show(media.data, keyframe: keyframe, from: media.producer)
                case .audio: self.sound.play(media.data, from: media.producer)
                }
            }
        }

        thread.name = "unkvoid-media"
        thread.qualityOfService = .userInteractive
        thread.start()
    }

    func stop() {
        gate.lock()
        running = false
        sinks = [:]
        backlog = [:]
        gate.unlock()

        sound.forget(nil)
        sound.mute()
        camera.stop()
    }

    private var isRunning: Bool {
        gate.lock()

        defer { gate.unlock() }

        return running
    }
}
