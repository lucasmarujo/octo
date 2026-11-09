import AVFoundation
import ExpoModulesCore

final class OctoScannerView: ExpoView, AVCaptureMetadataOutputObjectsDelegate {
  private static let repeatAfter: TimeInterval = 2
  private static let denied = "denied"
  private static let unavailable = "unavailable"

  let onScanned = EventDispatcher()
  let onFailed = EventDispatcher()

  private let capture = AVCaptureSession()
  private let captureQueue = DispatchQueue(label: "dev.marujo.octo.scanner")
  private let previewLayer: AVCaptureVideoPreviewLayer
  private var configured = false
  private var lastText = ""
  private var lastAt = Date.distantPast

  required init(appContext: AppContext? = nil) {
    previewLayer = AVCaptureVideoPreviewLayer(session: capture)
    super.init(appContext: appContext)
    clipsToBounds = true
    backgroundColor = .black
    previewLayer.videoGravity = .resizeAspectFill
    layer.addSublayer(previewLayer)
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    previewLayer.frame = bounds
  }

  override func didMoveToWindow() {
    super.didMoveToWindow()
    if window == nil {
      stop()
    } else {
      start()
    }
  }

  func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput metadataObjects: [AVMetadataObject], from connection: AVCaptureConnection) {
    guard let code = metadataObjects.first as? AVMetadataMachineReadableCodeObject, let text = code.stringValue else {
      return
    }
    let now = Date()
    if text == lastText && now.timeIntervalSince(lastAt) < OctoScannerView.repeatAfter {
      return
    }
    lastText = text
    lastAt = now
    onScanned(["text": text])
  }

  private func start() {
    guard AVCaptureDevice.authorizationStatus(for: .video) == .authorized else {
      onFailed(["reason": OctoScannerView.denied])
      return
    }
    captureQueue.async {
      if !self.configured && !self.configure() {
        DispatchQueue.main.async {
          self.onFailed(["reason": OctoScannerView.unavailable])
        }
        return
      }
      if !self.capture.isRunning {
        self.capture.startRunning()
      }
    }
  }

  private func stop() {
    captureQueue.async {
      if self.capture.isRunning {
        self.capture.stopRunning()
      }
    }
  }

  private func configure() -> Bool {
    guard let camera = AVCaptureDevice.default(for: .video), let input = try? AVCaptureDeviceInput(device: camera) else {
      return false
    }
    let output = AVCaptureMetadataOutput()
    guard capture.canAddInput(input), capture.canAddOutput(output) else {
      return false
    }
    capture.beginConfiguration()
    capture.addInput(input)
    capture.addOutput(output)
    capture.commitConfiguration()
    guard output.availableMetadataObjectTypes.contains(.qr) else {
      return false
    }
    output.setMetadataObjectsDelegate(self, queue: .main)
    output.metadataObjectTypes = [.qr]
    configured = true
    return true
  }
}
