import AVFoundation
import ExpoModulesCore

public class OctoScannerModule: Module {
  public func definition() -> ModuleDefinition {
    Name("OctoScanner")

    AsyncFunction("requestAccess") { (promise: Promise) in
      AVCaptureDevice.requestAccess(for: .video) { granted in
        promise.resolve(granted)
      }
    }

    View(OctoScannerView.self) {
      Events("onScanned", "onFailed")
    }
  }
}
