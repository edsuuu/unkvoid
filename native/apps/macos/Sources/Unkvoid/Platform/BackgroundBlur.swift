import CoreImage
import CoreVideo
import Vision

/// O primeiro filtro da câmera: a pessoa nítida e o fundo desfocado.
///
/// Tudo na placa de vídeo, e antes do encoder — a sala inteira vê o fundo desfocado, não só
/// quem liga o filtro. O resultado sai num `CVPixelBuffer` com `IOSurface` por trás, do mesmo
/// formato em que a câmera entrega: o encoder não percebe a diferença.
///
/// A qualidade vem de três escolhas. O recorte é o `accurate` do Vision, que pega fone, cabelo
/// e ombro que o `balanced` come. A borda do recorte é suavizada, para não virar serrilhado.
/// E o fundo é desfocado **sem a pessoa dentro** (o quadro vezes o inverso da máscara,
/// desfocado, dividido pela máscara desfocada): borrar o quadro inteiro espalharia a cor da
/// roupa e da pele num halo em volta dela.
///
/// O `accurate` não fecha 30 recortes por segundo em toda máquina, então ele roda numa fila
/// à parte e cada quadro é misturado com o recorte mais recente: a imagem nunca perde quadro,
/// e o recorte se atrasa no máximo um ou dois.
final class BackgroundBlur: @unchecked Sendable {
    private let request: VNGeneratePersonSegmentationRequest = {
        let request = VNGeneratePersonSegmentationRequest()

        request.qualityLevel = .accurate
        request.outputPixelFormat = kCVPixelFormatType_OneComponent8

        return request
    }()

    private let sequence = VNSequenceRequestHandler()
    private let cutting = DispatchQueue(label: "unkvoid-camera-cutout", qos: .userInteractive)
    private let context = CIContext(options: [.cacheIntermediates: false])
    private let guarded = NSLock()
    private var cutout: CVPixelBuffer?
    private var busy = false
    private var pool: CVPixelBufferPool?

    private static let blurRadius = 24.0
    private static let feather = 1.6

    /// Devolve o quadro com o fundo desfocado, ou `nil` se algo falhar — quem chama manda o
    /// quadro original, porque câmera sem filtro é melhor que câmera congelada.
    func blurred(_ frame: CVPixelBuffer) -> CVPixelBuffer? {
        guard let mask = latestCutout(for: frame), let output = buffer(like: frame) else {
            return nil
        }

        let image = CIImage(cvPixelBuffer: frame)
        let scale = CGAffineTransform(
            scaleX: image.extent.width / CGFloat(CVPixelBufferGetWidth(mask)),
            y: image.extent.height / CGFloat(CVPixelBufferGetHeight(mask))
        )
        let person = CIImage(cvPixelBuffer: mask)
            .transformed(by: scale)
            .clampedToExtent()
            .applyingGaussianBlur(sigma: Self.feather)
            .cropped(to: image.extent)
        let room = person.applyingFilter("CIColorInvert")

        let emptied = image.applyingFilter("CIMultiplyCompositing", parameters: [kCIInputBackgroundImageKey: room])
        let spread = emptied.clampedToExtent().applyingGaussianBlur(sigma: Self.blurRadius).cropped(to: image.extent)
        let weight = room.clampedToExtent().applyingGaussianBlur(sigma: Self.blurRadius).cropped(to: image.extent)

        // `CIDivideBlendMode` divide o fundo pela imagem de entrada: o fundo esvaziado e
        // desfocado, dividido por quanto de fundo havia em cada ponto.
        let background = weight.applyingFilter("CIDivideBlendMode", parameters: [kCIInputBackgroundImageKey: spread])

        let mixed = image.applyingFilter("CIBlendWithMask", parameters: [
            kCIInputBackgroundImageKey: background,
            kCIInputMaskImageKey: person,
        ])

        context.render(mixed, to: output)

        return output
    }

    /// O recorte mais recente, e o pedido do próximo se a fila estiver livre. O primeiro é
    /// feito na hora: sem ele o quadro sairia sem filtro, e o fundo apareceria por um instante.
    private func latestCutout(for frame: CVPixelBuffer) -> CVPixelBuffer? {
        guarded.lock()

        let known = cutout
        let free = !busy

        if free {
            busy = true
        }

        guarded.unlock()

        guard let known else {
            let first = cut(frame)

            finish(with: first)

            return first
        }

        if free {
            cutting.async { self.finish(with: self.cut(frame)) }
        }

        return known
    }

    private func cut(_ frame: CVPixelBuffer) -> CVPixelBuffer? {
        (try? sequence.perform([request], on: frame)).flatMap { request.results?.first?.pixelBuffer }
    }

    private func finish(with fresh: CVPixelBuffer?) {
        guarded.lock()
        cutout = fresh ?? cutout
        busy = false
        guarded.unlock()
    }

    private func buffer(like frame: CVPixelBuffer) -> CVPixelBuffer? {
        if pool == nil {
            let attributes: [String: Any] = [
                kCVPixelBufferPixelFormatTypeKey as String: CVPixelBufferGetPixelFormatType(frame),
                kCVPixelBufferWidthKey as String: CVPixelBufferGetWidth(frame),
                kCVPixelBufferHeightKey as String: CVPixelBufferGetHeight(frame),
                kCVPixelBufferIOSurfacePropertiesKey as String: [String: Any](),
            ]

            CVPixelBufferPoolCreate(nil, nil, attributes as CFDictionary, &pool)
        }

        var output: CVPixelBuffer?

        if let pool {
            CVPixelBufferPoolCreatePixelBuffer(nil, pool, &output)
        }

        return output
    }
}
