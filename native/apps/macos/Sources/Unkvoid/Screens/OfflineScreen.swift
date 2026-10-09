import SwiftUI

struct OfflineScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        SurfaceCard(width: 420) {
            VStack(spacing: 14) {
                Image(systemName: "wifi.slash")
                    .font(.system(size: 30))
                    .foregroundStyle(Theme.danger)

                Text("Sem conexão")
                    .font(.system(size: 18, weight: .semibold))

                Text(model.offlineStatus)
                    .font(.system(size: 13))
                    .foregroundStyle(Theme.inkSoft)
                    .multilineTextAlignment(.center)

                Button("Tentar de novo") {
                    Task { await model.retry() }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .tint(Theme.brand)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
