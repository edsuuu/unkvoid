import SwiftUI

/// `ShareModal.tsx`: escolher a tela ou o aplicativo — cada um com a miniatura que o núcleo
/// tira —, a qualidade, e transmitir.
struct ShareModal: View {
    @EnvironmentObject private var model: AppModel

    private static let qualities = ["720", "1080", "1440", "2160"]
    private static let frameRates = [15, 30, 60]

    var body: some View {
        ModalFrame(
            title: model.mine.sharing ? "Mudar a transmissão" : "Compartilhar tela",
            subtitle: "Escolha o que a sala vai ver.",
            width: 560,
            onClose: { model.shareOpen = false }
        ) {
            VStack(alignment: .leading, spacing: 16) {
                HStack(spacing: 6) {
                    tab("Telas", "display")
                    tab("Aplicativos", "window")
                }

                sources
                    .frame(height: 340, alignment: .top)

                VStack(alignment: .leading, spacing: 10) {
                    Toggle("Transmitir o áudio", isOn: $model.shareAudio)

                    Toggle("Sem o áudio do Discord", isOn: $model.shareMuteCalls)
                        .disabled(!model.shareAudio)
                        .opacity(model.shareAudio ? 1 : 0.5)
                }
                .toggleStyle(.checkbox)
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkIcon)
                .tint(Theme.brand)
            }
        } footer: {
            picker("Qualidade", selection: $model.shareQuality, options: Self.qualities) { $0 == "2160" ? "4K (2160p)" : "\($0)p" }

            picker("FPS", selection: $model.shareFps, options: Self.frameRates) { "\($0)" }

            Spacer(minLength: 0)

            Button("Cancelar") {
                model.shareOpen = false
            }
            .buttonStyle(.pointer)
            .font(Theme.sans(13.5))
            .foregroundStyle(Theme.inkDim)
            .padding(.horizontal, 14)

            Button("Transmitir") {
                Task { await model.confirmShare() }
            }
            .buttonStyle(PrimaryButton())
            .font(Theme.sans(13.5, .semibold))
            .disabled(model.shareSource == nil)
        }
    }

    @ViewBuilder
    private var sources: some View {
        let items = model.shareTab == "window" ? model.shareWindows : model.shareDisplays

        if model.shareLoading {
            Text("Procurando o que dá para compartilhar…")
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkSoft)
        } else if items.isEmpty {
            Text(
                model.shareTab == "window"
                    ? "Nenhuma janela aberta para compartilhar."
                    : "Nenhuma tela encontrada. No macOS, autorize a gravação de tela nas Configurações do Sistema."
            )
            .font(Theme.sans(13))
            .foregroundStyle(Theme.inkSoft)
        } else {
            ScrollView {
                LazyVGrid(columns: columns(for: items.count), spacing: 12) {
                    ForEach(items) { item in
                        source(item)
                    }
                }
                // Uma tela só cresce até onde ainda cabe inteira nos 340 do painel, sem rolar.
                .frame(maxWidth: items.count == 1 ? 440 : .infinity)
                .frame(maxWidth: .infinity)
            }
        }
    }

    /// O `auto-fit` do CSS estica o que houver até encher a linha; o `.adaptive` do SwiftUI
    /// não. Com uma ou duas origens cada uma ganha a sua coluna inteira, e só de três em
    /// diante os cartões encolhem para caber mais por linha.
    private func columns(for count: Int) -> [GridItem] {
        count <= 2
            ? Array(repeating: GridItem(.flexible(), spacing: 12), count: max(count, 1))
            : [GridItem(.adaptive(minimum: 150), spacing: 12)]
    }

    private func source(_ item: ShareSource) -> some View {
        let chosen = model.shareSource == item.id

        return Button {
            model.shareSource = item.id
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                ZStack {
                    LinearGradient(colors: [Theme.brandDark.opacity(0.3), .black], startPoint: .topLeading, endPoint: .bottomTrailing)

                    if let preview = model.sharePreviews[item.id] {
                        Image(nsImage: preview)
                            .resizable()
                            .scaledToFit()
                    } else {
                        Text("sem prévia")
                            .font(Theme.mono(10))
                            .foregroundStyle(Theme.inkDim)
                    }
                }
                .aspectRatio(16 / 10, contentMode: .fit)

                VStack(alignment: .leading, spacing: 2) {
                    Text(item.label)
                        .font(Theme.sans(13.5, .medium))
                        .foregroundStyle(Theme.ink)
                        .lineLimit(1)

                    Text(item.detail)
                        .font(Theme.mono(11))
                        .foregroundStyle(Theme.inkDim)
                        .lineLimit(1)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(Theme.fieldFill)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(chosen ? Theme.brand.opacity(0.45) : Theme.line, lineWidth: 1)
            )
        }
        .buttonStyle(.pointer)
    }

    private func tab(_ label: String, _ value: String) -> some View {
        let chosen = model.shareTab == value

        return Button(label) {
            model.shareTab = value
        }
        .buttonStyle(.pointer)
        .font(Theme.sans(12.5, .medium))
        .foregroundStyle(chosen ? Theme.inkStrong : Theme.inkDim)
        .padding(.horizontal, 12)
        .padding(.vertical, 7)
        .background(chosen ? Theme.row : .clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
    }

    private func picker<Option: Hashable>(
        _ label: String,
        selection: Binding<Option>,
        options: [Option],
        title: @escaping (Option) -> String
    ) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).labelMono()

            Picker(label, selection: selection) {
                ForEach(options, id: \.self) { option in
                    Text(title(option)).tag(option)
                }
            }
            .labelsHidden()
            .fixedSize()
        }
    }
}
