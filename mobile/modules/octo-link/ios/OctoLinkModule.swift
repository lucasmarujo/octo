import ExpoModulesCore

public class OctoLinkModule: Module {
  private static let ready = "onReady"
  private static let message = "onMessage"
  private static let closed = "onClosed"

  private let lock = NSLock()
  private var sessions: [String: Session] = [:]

  public func definition() -> ModuleDefinition {
    Name("OctoLink")

    Events(OctoLinkModule.ready, OctoLinkModule.message, OctoLinkModule.closed)

    Function("open") { (id: String, url: String, key: String, agent: String) in
      self.open(id: id, url: url, key: key, agent: agent)
    }

    Function("send") { (id: String, text: String) in
      self.session(id)?.send(text)
    }

    Function("close") { (id: String) in
      self.take(id)?.close()
    }

    OnDestroy {
      for session in self.takeAll() {
        session.close()
      }
    }
  }

  private func open(id: String, url: String, key: String, agent: String) {
    guard let target = URL(string: url), let secret = Base64Url.decode(key), secret.count == Channel.secretLength else {
      sendEvent(OctoLinkModule.closed, ["id": id, "code": Session.abnormal, "reason": "the pairing key is not valid"])
      return
    }
    let events = Session.Events(
      ready: { [weak self] id in
        self?.sendEvent(OctoLinkModule.ready, ["id": id])
      },
      message: { [weak self] id, text in
        self?.sendEvent(OctoLinkModule.message, ["id": id, "text": text])
      },
      closed: { [weak self] id, code, reason in
        _ = self?.take(id)
        self?.sendEvent(OctoLinkModule.closed, ["id": id, "code": code, "reason": reason])
      }
    )
    let session = Session(id: id, secret: secret, events: events)
    lock.lock()
    let previous = sessions.updateValue(session, forKey: id)
    lock.unlock()
    previous?.close()
    session.start(url: target, agent: agent)
  }

  private func session(_ id: String) -> Session? {
    lock.lock()
    defer { lock.unlock() }
    return sessions[id]
  }

  private func take(_ id: String) -> Session? {
    lock.lock()
    defer { lock.unlock() }
    return sessions.removeValue(forKey: id)
  }

  private func takeAll() -> [Session] {
    lock.lock()
    defer { lock.unlock() }
    let all = Array(sessions.values)
    sessions.removeAll()
    return all
  }
}
