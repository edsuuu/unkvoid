import CoreAudio
import Foundation

/// Os microfones e as saídas que existem nesta máquina.
///
/// Quem responde é o CoreAudio, e não o núcleo: a lista muda quando o fone é plugado, é
/// diferente em cada sistema e não atravessa a rede. É o mesmo papel que o
/// `enumerateDevices()` faz no app do navegador.
struct AudioDevice: Identifiable, Hashable, Sendable {
    let id: AudioDeviceID
    let name: String
}

enum Audio {
    static func inputs() -> [AudioDevice] {
        devices(scope: kAudioObjectPropertyScopeInput)
    }

    static func outputs() -> [AudioDevice] {
        devices(scope: kAudioObjectPropertyScopeOutput)
    }

    private static func devices(scope: AudioObjectPropertyScope) -> [AudioDevice] {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var size: UInt32 = 0

        guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size) == noErr else {
            return []
        }

        var ids = [AudioDeviceID](repeating: 0, count: Int(size) / MemoryLayout<AudioDeviceID>.size)

        guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &ids) == noErr else {
            return []
        }

        return ids.compactMap { device in
            guard channels(of: device, scope: scope) > 0, let name = name(of: device) else {
                return nil
            }

            return AudioDevice(id: device, name: name)
        }
    }

    /// Entrada e saída são o mesmo objeto no CoreAudio: o que separa as duas listas é de
    /// que lado o aparelho tem canal.
    private static func channels(of device: AudioDeviceID, scope: AudioObjectPropertyScope) -> Int {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain
        )
        var size: UInt32 = 0

        guard AudioObjectGetPropertyDataSize(device, &address, 0, nil, &size) == noErr, size > 0 else {
            return 0
        }

        let buffer = UnsafeMutableRawPointer.allocate(byteCount: Int(size), alignment: MemoryLayout<AudioBufferList>.alignment)

        defer { buffer.deallocate() }

        guard AudioObjectGetPropertyData(device, &address, 0, nil, &size, buffer) == noErr else {
            return 0
        }

        let list = UnsafeMutableAudioBufferListPointer(buffer.assumingMemoryBound(to: AudioBufferList.self))

        return list.reduce(0) { $0 + Int($1.mNumberChannels) }
    }

    private static func name(of device: AudioDeviceID) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioObjectPropertyName,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var name: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)

        guard AudioObjectGetPropertyData(device, &address, 0, nil, &size, &name) == noErr, let name else {
            return nil
        }

        return name.takeRetainedValue() as String
    }
}
