import SwiftUI

/// O aviso curto que some sozinho — o `Toasts.tsx` do React.
struct Notice: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let notice = model.notice {
            Text(notice)
                .font(Theme.sans(13))
                .foregroundStyle(Theme.inkBody)
                .padding(.vertical, 10)
                .padding(.horizontal, 16)
                .popoverPanel()
                .padding(.top, 8)
                .onTapGesture { model.dismissNotice() }
                .task(id: notice) {
                    try? await Task.sleep(for: .seconds(4))
                    model.dismissNotice()
                }
        }
    }
}
