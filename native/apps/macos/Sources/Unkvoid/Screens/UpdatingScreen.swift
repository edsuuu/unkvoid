import SwiftUI

struct UpdatingScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 16) {
            ProgressView()
                .controlSize(.large)

            Text(model.updateStatus)
                .font(.system(size: 13))
                .foregroundStyle(Theme.soft)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
