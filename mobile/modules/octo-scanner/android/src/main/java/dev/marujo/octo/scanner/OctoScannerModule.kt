package dev.marujo.octo.scanner

import android.Manifest
import expo.modules.interfaces.permissions.PermissionsStatus
import expo.modules.kotlin.Promise
import expo.modules.kotlin.exception.Exceptions
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class OctoScannerModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("OctoScanner")

        AsyncFunction("requestAccess") { promise: Promise ->
            val permissions = appContext.permissions ?: throw Exceptions.AppContextLost()
            permissions.askForPermissions(
                { result -> promise.resolve(result[Manifest.permission.CAMERA]?.status == PermissionsStatus.GRANTED) },
                Manifest.permission.CAMERA,
            )
        }

        View(OctoScannerView::class) {
            Events("onScanned", "onFailed")
        }
    }
}
