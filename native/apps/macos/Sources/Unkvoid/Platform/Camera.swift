import AVFoundation
import CoreVideo

/// A câmera, capturada pelo sistema e entregue ao núcleo no buffer de GPU em que veio.
///
/// O `AVCaptureSession` devolve cada quadro num `CVPixelBuffer` apoiado em `IOSurface` — a
/// mesma coisa que o ScreenCaptureKit dá para a tela. É o `IOSurface` que atravessa a ABI,
/// então o quadro vai do sensor ao encoder da placa sem passar pela memória do processador.
final class Camera: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate, @unchecked Sendable {
    static let size = (width: 1280, height: 720)
    static let frameRate = 30

    private let session = AVCaptureSession()
    private let frames = DispatchQueue(label: "unkvoid-camera")
    private var seen: (@Sendable (IOSurfaceRef, UInt64) -> Void)?
    private var blur: BackgroundBlur?

    /// Liga e desliga o desfoque do fundo, com a câmera no ar ou não. O filtro só existe
    /// enquanto está ligado: desligado, o quadro segue do sensor ao encoder sem ser tocado.
    func blurBackground(_ wanted: Bool) {
        frames.async { self.blur = wanted ? self.blur ?? BackgroundBlur() : nil }
    }

    enum Failure: Error {
        case noCamera
        case notAllowed
    }

    /// Liga a câmera padrão e entrega cada quadro, com o tempo em nanossegundos, a `seen`.
    func start(_ seen: @escaping @Sendable (IOSurfaceRef, UInt64) -> Void) async throws {
        guard await AVCaptureDevice.requestAccess(for: .video) else {
            throw Failure.notAllowed
        }

        guard let device = AVCaptureDevice.default(for: .video), let input = try? AVCaptureDeviceInput(device: device) else {
            throw Failure.noCamera
        }

        let output = AVCaptureVideoDataOutput()

        // NV12 com `IOSurface` por trás: é o que o VideoToolbox come sem converter.
        output.videoSettings = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferIOSurfacePropertiesKey as String: [String: Any](),
            kCVPixelBufferWidthKey as String: Self.size.width,
            kCVPixelBufferHeightKey as String: Self.size.height,
        ]
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(self, queue: frames)

        session.beginConfiguration()
        session.sessionPreset = .hd1280x720

        guard session.canAddInput(input), session.canAddOutput(output) else {
            session.commitConfiguration()

            throw Failure.noCamera
        }

        session.addInput(input)
        session.addOutput(output)
        session.commitConfiguration()

        frames.sync { self.seen = seen }
        session.startRunning()
    }

    func stop() {
        session.stopRunning()
        session.inputs.forEach(session.removeInput)
        session.outputs.forEach(session.removeOutput)
        frames.sync { seen = nil }
    }

    func captureOutput(_: AVCaptureOutput, didOutput sample: CMSampleBuffer, from _: AVCaptureConnection) {
        guard let seen, let captured = CMSampleBufferGetImageBuffer(sample) else {
            return
        }

        let pixels = blur?.blurred(captured) ?? captured

        guard let surface = CVPixelBufferGetIOSurface(pixels)?.takeUnretainedValue() else {
            return
        }

        let time = CMSampleBufferGetPresentationTimeStamp(sample)

        seen(surface, UInt64(max(0, time.seconds) * 1_000_000_000))
    }
}
