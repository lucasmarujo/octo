import CryptoKit
import Foundation

final class Session: NSObject, URLSessionWebSocketDelegate {
  struct Events {
    let ready: (String) -> Void
    let message: (String, String) -> Void
    let closed: (String, Int, String) -> Void
  }

  static let abnormal = 1006
  static let outdated = 4002
  private static let connectSeconds: TimeInterval = 10
  private static let pingSeconds = 20
  private static let userAgent = "User-Agent"
  private static let hello = "hello"
  private static let sync = Data("{\"type\":\"sync\"}".utf8)

  private let id: String
  private let secret: Data
  private let events: Events
  private let queue: DispatchQueue
  private var network: URLSession?
  private var task: URLSessionWebSocketTask?
  private var own: P256.KeyAgreement.PrivateKey?
  private var channel: Channel?
  private var pings: DispatchSourceTimer?
  private var awaitingPong = false
  private var finished = false

  init(id: String, secret: Data, events: Events) {
    self.id = id
    self.secret = secret
    self.events = events
    queue = DispatchQueue(label: "dev.marujo.octo.link.\(id)")
  }

  func start(url: URL, agent: String) {
    queue.async {
      var request = URLRequest(url: url)
      request.timeoutInterval = Session.connectSeconds
      request.setValue(agent, forHTTPHeaderField: Session.userAgent)
      let callbacks = OperationQueue()
      callbacks.underlyingQueue = self.queue
      callbacks.maxConcurrentOperationCount = 1
      let network = URLSession(configuration: .ephemeral, delegate: self, delegateQueue: callbacks)
      let task = network.webSocketTask(with: request)
      self.own = P256.KeyAgreement.PrivateKey()
      self.network = network
      self.task = task
      task.resume()
      self.listen()
    }
  }

  func send(_ text: String) {
    queue.async {
      guard let channel = self.channel, !self.finished else {
        return
      }
      do {
        let frame = try channel.seal(Data(text.utf8))
        self.task?.send(.data(frame)) { [weak self] error in
          if let error {
            self?.queue.async { self?.finish(Session.abnormal, error.localizedDescription) }
          }
        }
      } catch {
        self.fail(error)
      }
    }
  }

  func close() {
    queue.async {
      guard !self.finished else {
        return
      }
      self.finished = true
      self.task?.cancel(with: .normalClosure, reason: nil)
      self.release()
    }
  }

  func urlSession(_ session: URLSession, webSocketTask: URLSessionWebSocketTask, didOpenWithProtocol protocol: String?) {
    guard let own, !finished else {
      return
    }
    let key = Base64Url.encode(own.publicKey.x963Representation)
    let greeting = "{\"type\":\"\(Session.hello)\",\"version\":\(Channel.version),\"key\":\"\(key)\"}"
    webSocketTask.send(.string(greeting)) { [weak self] error in
      if let error {
        self?.queue.async { self?.finish(Session.abnormal, error.localizedDescription) }
      }
    }
    startPings()
  }

  func urlSession(_ session: URLSession, webSocketTask: URLSessionWebSocketTask, didCloseWith closeCode: URLSessionWebSocketTask.CloseCode, reason: Data?) {
    finish(closeCode.rawValue, reason.flatMap { String(data: $0, encoding: .utf8) } ?? "")
  }

  func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
    if let error {
      drop(error)
    }
  }

  private func listen() {
    task?.receive { [weak self] result in
      guard let self else {
        return
      }
      self.queue.async {
        guard !self.finished else {
          return
        }
        switch result {
        case .success(.string(let text)):
          self.greet(text)
        case .success(.data(let frame)):
          self.read(frame)
        case .success:
          break
        case .failure(let error):
          self.drop(error)
          return
        }
        self.listen()
      }
    }
  }

  private func greet(_ text: String) {
    guard channel == nil, let own else {
      return
    }
    guard let peer = Session.peerKey(text) else {
      task?.cancel(with: .normalClosure, reason: nil)
      finish(Session.outdated, "")
      return
    }
    do {
      let established = try Channel(own: own, peer: peer, secret: secret)
      let first = try established.seal(Session.sync)
      self.own = nil
      channel = established
      task?.send(.data(first)) { [weak self] error in
        if let error {
          self?.queue.async { self?.finish(Session.abnormal, error.localizedDescription) }
        }
      }
      events.ready(id)
    } catch {
      fail(error)
    }
  }

  private func read(_ frame: Data) {
    guard let channel else {
      fail(ChannelError.frame)
      return
    }
    do {
      let plain = try channel.open(frame)
      events.message(id, String(decoding: plain, as: UTF8.self))
    } catch {
      fail(error)
    }
  }

  private func startPings() {
    let timer = DispatchSource.makeTimerSource(queue: queue)
    timer.schedule(deadline: .now() + .seconds(Session.pingSeconds), repeating: .seconds(Session.pingSeconds))
    timer.setEventHandler { [weak self] in
      self?.ping()
    }
    pings = timer
    timer.resume()
  }

  private func ping() {
    guard !finished else {
      return
    }
    if awaitingPong {
      task?.cancel(with: .goingAway, reason: nil)
      finish(Session.abnormal, "the computer stopped answering")
      return
    }
    awaitingPong = true
    task?.sendPing { [weak self] error in
      self?.queue.async {
        if let error {
          self?.finish(Session.abnormal, error.localizedDescription)
        } else {
          self?.awaitingPong = false
        }
      }
    }
  }

  private func drop(_ error: Error) {
    let code = task?.closeCode.rawValue ?? 0
    let reason = task?.closeReason.flatMap { String(data: $0, encoding: .utf8) }
    finish(code > 0 ? code : Session.abnormal, reason ?? error.localizedDescription)
  }

  private func fail(_ error: Error) {
    task?.cancel(with: .normalClosure, reason: nil)
    finish(Session.abnormal, error.localizedDescription)
  }

  private func finish(_ code: Int, _ reason: String) {
    guard !finished else {
      return
    }
    finished = true
    release()
    events.closed(id, code, reason)
  }

  private func release() {
    pings?.cancel()
    pings = nil
    network?.invalidateAndCancel()
    network = nil
  }

  private static func peerKey(_ text: String) -> Data? {
    guard
      let greeting = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any],
      greeting["type"] as? String == hello,
      greeting["version"] as? Int == Channel.version,
      let encoded = greeting["key"] as? String,
      let key = Base64Url.decode(encoded),
      key.count == Channel.publicKeyLength
    else {
      return nil
    }
    return key
  }
}
