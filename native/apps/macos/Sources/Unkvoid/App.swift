import SwiftUI

@main
struct UnkvoidApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @StateObject private var model = AppModel(url: Launch.socketUrl())

    var body: some Scene {
        WindowGroup("Unkvoid") {
            RootView()
                .environmentObject(model)
                .frame(minWidth: 960, minHeight: 640)
                .task { await model.start() }
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1180, height: 760)
    }
}

struct RootView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        ZStack {
            Theme.backdrop.ignoresSafeArea()

            switch model.screen {
            case .entry: EntryScreen()
            case .hub: HubScreen()
            case .room: RoomScreen()
            case .offline: OfflineScreen()
            case .updating: UpdatingScreen()
            }
        }
        .preferredColorScheme(.dark)
    }
}
