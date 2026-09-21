import SwiftUI

/// `ui/components/hub/HomeView.tsx`: a coluna das conversas à esquerda e, ao lado, a
/// conversa aberta, os amigos ou as salas.
struct HomeView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 12) {
            DirectColumn()

            if model.directPerson != nil {
                DirectPanel()
            } else if model.homeTab == .friends {
                FriendsPanel()
            } else {
                ServersHome()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
