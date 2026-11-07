import NshFirebase
import NativeShell
import UIKit

/// Registers the facade's `firebase.*` handlers on the app view. The package
/// itself cannot link NativeShell, so this glue lives in the host, with the
/// contract's `registerHandlers(on:)` shape.
extension NshFirebase {
    @MainActor
    public func registerHandlers(on app: NshAppView) {
        // Presses and other events queue in the view's channel until the
        // app listens, so wiring this before loadBundle loses nothing.
        sendEvent = { [weak app] name, data in app?.send(name, data) }
        for name in Self.handlerNames {
            app.handle(name) { [weak self] data, reply in
                guard let self else {
                    reply.reject("the NshFirebase facade is gone")
                    return
                }
                self.dispatch(name: name, data: data) { value, error in
                    if let error {
                        reply.reject(error)
                    } else {
                        reply.resolve(value)
                    }
                }
            }
        }
    }
}

final class HostController: UIViewController {
    private let app = NshAppView()
    private let status = UILabel()
    private var lines: [String] = []

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground

        let title = UILabel()
        title.text = "nsh-firebase host"
        title.font = .boldSystemFont(ofSize: 20)
        status.font = .systemFont(ofSize: 11)
        status.textColor = .secondaryLabel
        status.numberOfLines = 0
        let header = UIStackView(arrangedSubviews: [title, status])
        header.axis = .vertical
        header.alignment = .leading
        header.spacing = 6
        header.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(header)

        app.translatesAutoresizingMaskIntoConstraints = false
        app.onError = { [weak self] message in self?.log("nsh error: \(message)") }
        view.addSubview(app)

        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
            header.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 20),
            header.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -20),
            app.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 12),
            app.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 12),
            app.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -12),
            app.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -12),
        ])
        start()
    }

    private func start() {
        let firebase = NshFirebase.shared
        firebase.onActivity = { [weak self] line in self?.log(line) }
        do {
            guard
                let plist = Bundle.main.url(forResource: "GoogleService-Info", withExtension: "plist")
            else {
                log("no GoogleService-Info.plist in the app bundle")
                return
            }
            try firebase.configure(from: plist)
            firebase.registerHandlers(on: app)
            // Both sides must list the same names: the app's nsh.json
            // declares them, this grant allows them.
            try app.grantCapabilities(
                #"{ "network": { "hosts": ["127.0.0.1"] }, "host": ["firebase.*"] }"#)
            app.params = ["project": "demo-nsh"]
        } catch {
            log("\(error)")
            return
        }
        guard
            let bundle = Bundle.main.url(forResource: "bundle", withExtension: "nshc", subdirectory: "nsh")
        else {
            log("no nsh/bundle.nshc — build the mini-app first")
            return
        }
        do {
            try app.loadBundle(at: bundle)
            log("bundle loaded")
        } catch {
            log("\(error)")
        }
    }

    private func log(_ line: String) {
        lines.append(line)
        if lines.count > 8 { lines.removeFirst(lines.count - 8) }
        status.text = lines.joined(separator: "\n")
    }

    // Let the embedded app's pages and dialogs set the status bar.
    override var preferredStatusBarStyle: UIStatusBarStyle { NshAppView.preferredStatusBarStyle }
    override var prefersStatusBarHidden: Bool { NshAppView.prefersStatusBarHidden }
    override var preferredStatusBarUpdateAnimation: UIStatusBarAnimation {
        NshAppView.preferredStatusBarUpdateAnimation
    }
}
