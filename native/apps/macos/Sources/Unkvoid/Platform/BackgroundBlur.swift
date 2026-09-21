import CoreImage
import CoreVideo
import Vision

/// O primeiro filtro da câmera: a pessoa nítida e o fundo desfocado.
///
/// Tudo na placa de vídeo, e antes do encoder — a sala inteira vê o fundo desfocado, não só
/// quem liga o filtro. O Vision recorta a pessoa, o Core Image borra o quadro e mistura os
/// dois pela máscara, e o resultado sai num `CVPixelBuffer` com `IOSurface` por trás, do mesmo
/// formato em que a câmera entrega: o encoder não percebe a diferença.
final class BackgroundBlur {
    private let request: VNGeneratePersonSegmentationRequest = {
        let request = VNGeneratePersonSegmentationRequest()

        // `balanced` segura 30 quadros por segundo com folga; o `accurate` não segura.
        request.qualityLevel = .balanced
        request.outputPixelFormat = kCVPixelFormatType_OneComponent8

        return request
    }()

    private let sequence = VNSequenceRequestHandler()
    private let context = CIContext(options: [.cacheIntermediates: false])
    private var pool: CVPixelBufferPool?

    /// Devolve o quadro com o fundo desfocado, ou `nil` se algo falhar — quem chama manda o
    /// quadro original, porque câmera sem filtro é melhor que câmera congelada.
    func blurred(_ frame: CVPixelBuffer) -> CVPixelBuffer? {
        guard (try? sequence.perform([request], on: frame)) != nil, let mask = request.results?.first?.pixelBuffer, let output = buffer(like: frame) else {
            return nil
        }

        let image = CIImage(cvPixelBuffer: frame)
        let person = CIImage(cvPixelBuffer: mask)
        let scale = CGAffineTransform(scaleX: image.extent.width / person.extent.width, y: image.extent.height / person.extent.height)

        let background = image.clampedToExtent().applyingGaussianBlur(sigma: 18).cropped(to: image.extent)
        let mixed = image.applyingFilter("CIBlendWithMask", parameters: [
            kCIInputBackgroundImageKey: background,
            kCIInputMaskImageKey: person.transformed(by: scale),
        ])

        context.render(mixed, to: output)

        return output
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
