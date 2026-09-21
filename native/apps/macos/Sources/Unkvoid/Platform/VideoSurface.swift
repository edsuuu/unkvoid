import AVFoundation
import CoreImage
import CoreMedia
import SwiftUI

/// A tela de alguém, desenhada pelo decodificador do sistema.
///
/// O núcleo entrega o quadro H.264 em Annex-B; o `AVSampleBufferDisplayLayer` decodifica na
/// placa e desenha sozinho. O que sobra para cá é a tradução de formato: o VideoToolbox
/// quer o SPS e o PPS fora do fluxo e cada NAL com o tamanho na frente (AVCC), não o
/// código de início.
final class VideoSink: @unchecked Sendable {
    /// Só a main mexe no layer; quem recebe quadro de outra thread é o `renderer` dele.
    @MainActor let layer: AVSampleBufferDisplayLayer

    private let renderer: AVSampleBufferVideoRenderer
    private let gate = NSLock()
    private var format: CMVideoFormatDescription?

    @MainActor
    init() {
        layer = AVSampleBufferDisplayLayer()
        layer.videoGravity = .resizeAspect
        layer.backgroundColor = NSColor.black.cgColor
        renderer = layer.sampleBufferRenderer
    }

    /// Chamado da thread da mídia, fora da main.
    func show(_ annexB: Data, keyframe: Bool) {
        gate.lock()

        defer { gate.unlock() }

        let nals = Self.nals(annexB)

        if keyframe, let described = Self.describe(nals) {
            format = described
        }

        // Layer que falhou só volta com `flush`, e depois dele só um keyframe desenha.
        if renderer.status == .failed {
            renderer.flush()

            guard keyframe else {
                return
            }
        }

        guard let format, let sample = Self.sample(nals, format) else {
            return
        }

        renderer.enqueue(sample)
    }

    private static let parameterSet: Set<UInt8> = [7, 8]

    /// Onde cada NAL começa e termina, sem o código de início na frente.
    static func nals(_ annexB: Data) -> [Data] {
        let bytes = [UInt8](annexB)
        var starts: [Int] = []
        var index = 0

        while index + 2 < bytes.count {
            if bytes[index] == 0, bytes[index + 1] == 0, bytes[index + 2] == 1 {
                starts.append(index)
                index += 3
            } else {
                index += 1
            }
        }

        return starts.enumerated().compactMap { position, start in
            var end = position + 1 < starts.count ? starts[position + 1] : bytes.count

            // Um NAL nunca termina em zero: o que sobra é o começo do código de 4 bytes seguinte.
            while end > start + 3, bytes[end - 1] == 0 {
                end -= 1
            }

            return end > start + 3 ? Data(bytes[(start + 3) ..< end]) : nil
        }
    }

    private static func describe(_ nals: [Data]) -> CMVideoFormatDescription? {
        guard
            let sps = nals.first(where: { $0[$0.startIndex] & 0x1F == 7 }),
            let pps = nals.first(where: { $0[$0.startIndex] & 0x1F == 8 })
        else {
            return nil
        }

        var format: CMVideoFormatDescription?

        let status = sps.withUnsafeBytes { spsBytes in
            pps.withUnsafeBytes { ppsBytes in
                let sets = [
                    spsBytes.bindMemory(to: UInt8.self).baseAddress!,
                    ppsBytes.bindMemory(to: UInt8.self).baseAddress!,
                ]

                return CMVideoFormatDescriptionCreateFromH264ParameterSets(
                    allocator: nil,
                    parameterSetCount: 2,
                    parameterSetPointers: sets,
                    parameterSetSizes: [sps.count, pps.count],
                    nalUnitHeaderLength: 4,
                    formatDescriptionOut: &format
                )
            }
        }

        return status == noErr ? format : nil
    }

    /// Um quadro em AVCC, marcado para desenhar na hora: ao vivo não há relógio para
    /// respeitar, e o quadro que chegou é o que se mostra.
    private static func sample(_ nals: [Data], _ format: CMVideoFormatDescription) -> CMSampleBuffer? {
        var avcc = Data()

        for nal in nals where !parameterSet.contains(nal[nal.startIndex] & 0x1F) {
            var length = UInt32(nal.count).bigEndian

            avcc.append(Data(bytes: &length, count: 4))
            avcc.append(nal)
        }

        guard !avcc.isEmpty else {
            return nil
        }

        var block: CMBlockBuffer?

        guard
            CMBlockBufferCreateWithMemoryBlock(
                allocator: nil,
                memoryBlock: nil,
                blockLength: avcc.count,
                blockAllocator: nil,
                customBlockSource: nil,
                offsetToData: 0,
                dataLength: avcc.count,
                flags: 0,
                blockBufferOut: &block
            ) == noErr,
            let block,
            avcc.withUnsafeBytes({ CMBlockBufferReplaceDataBytes(with: $0.baseAddress!, blockBuffer: block, offsetIntoDestination: 0, dataLength: avcc.count) }) == noErr
        else {
            return nil
        }

        var sample: CMSampleBuffer?
        var size = avcc.count

        guard
            CMSampleBufferCreateReady(
                allocator: nil,
                dataBuffer: block,
                formatDescription: format,
                sampleCount: 1,
                sampleTimingEntryCount: 0,
                sampleTimingArray: nil,
                sampleSizeEntryCount: 1,
                sampleSizeArray: &size,
                sampleBufferOut: &sample
            ) == noErr,
            let sample
        else {
            return nil
        }

        if let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: true) as? [NSMutableDictionary] {
            attachments.first?[kCMSampleAttachmentKey_DisplayImmediately] = true
        }

        return sample
    }
}

/// O layer do `VideoSink` dentro do SwiftUI, com o ajuste de imagem de quem assiste.
struct VideoSurface: NSViewRepresentable {
    let sink: VideoSink
    var look = ImageFilters.Look()

    func makeNSView(context: Context) -> NSView {
        let view = NSView()

        view.wantsLayer = true
        view.layerUsesCoreImageFilters = true
        view.layer = CALayer()
        view.layer?.backgroundColor = NSColor.black.cgColor
        attach(to: view)

        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        if sink.layer.superlayer !== view.layer {
            attach(to: view)
        }

        sink.layer.frame = view.bounds
        sink.layer.filters = filters
    }

    /// Os mesmos quatro controles do CSS (`brightness`, `contrast`, `saturate`, `blur`), em
    /// Core Image. Sem ajuste nenhum o layer fica sem filtro: filtro é custo por quadro.
    private var filters: [CIFilter] {
        guard !look.untouched else {
            return []
        }

        var applied: [CIFilter] = []

        if let color = CIFilter(name: "CIColorControls") {
            color.setValue((look.brightness - 100) / 100 * 0.5, forKey: kCIInputBrightnessKey)
            color.setValue(look.contrast / 100, forKey: kCIInputContrastKey)
            color.setValue(look.saturation / 100, forKey: kCIInputSaturationKey)
            applied.append(color)
        }

        if look.blur > 0, let blur = CIFilter(name: "CIGaussianBlur") {
            blur.setValue(look.blur, forKey: kCIInputRadiusKey)
            applied.append(blur)
        }

        return applied
    }

    private func attach(to view: NSView) {
        sink.layer.removeFromSuperlayer()
        sink.layer.frame = view.bounds
        sink.layer.autoresizingMask = [.layerWidthSizable, .layerHeightSizable]
        view.layer?.addSublayer(sink.layer)
    }
}
