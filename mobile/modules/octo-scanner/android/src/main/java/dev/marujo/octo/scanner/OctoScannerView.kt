package dev.marujo.octo.scanner

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.SystemClock
import android.view.ViewGroup
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import com.google.zxing.BinaryBitmap
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.viewevent.EventDispatcher
import expo.modules.kotlin.views.ExpoView
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

class OctoScannerView(context: Context, appContext: AppContext) : ExpoView(context, appContext) {
    private val onScanned by EventDispatcher<Map<String, String>>()
    private val onFailed by EventDispatcher<Map<String, String>>()
    private val analysis: ExecutorService = Executors.newSingleThreadExecutor()
    private val preview = PreviewView(context).apply {
        implementationMode = PreviewView.ImplementationMode.COMPATIBLE
        scaleType = PreviewView.ScaleType.FILL_CENTER
    }
    private var provider: ProcessCameraProvider? = null
    private var lastText = ""
    private var lastAt = 0L

    override val shouldUseAndroidLayout = true

    init {
        addView(preview, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        start()
    }

    override fun onDetachedFromWindow() {
        provider?.unbindAll()
        provider = null
        super.onDetachedFromWindow()
    }

    private fun start() {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            onFailed(mapOf(REASON to DENIED))
            return
        }
        val owner = appContext.currentActivity as? LifecycleOwner
        if (owner == null) {
            onFailed(mapOf(REASON to UNAVAILABLE))
            return
        }
        val future = ProcessCameraProvider.getInstance(context)
        future.addListener({
            try {
                val cameras = future.get()
                val view = Preview.Builder().build().also { it.surfaceProvider = preview.surfaceProvider }
                val frames = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
                frames.setAnalyzer(analysis, QrAnalyzer { text -> post { deliver(text) } })
                cameras.unbindAll()
                cameras.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, view, frames)
                provider = cameras
            } catch (error: ExecutionException) {
                onFailed(mapOf(REASON to UNAVAILABLE))
            } catch (error: IllegalArgumentException) {
                onFailed(mapOf(REASON to UNAVAILABLE))
            } catch (error: IllegalStateException) {
                onFailed(mapOf(REASON to UNAVAILABLE))
            }
        }, ContextCompat.getMainExecutor(context))
    }

    private fun deliver(text: String) {
        val now = SystemClock.elapsedRealtime()
        if (text == lastText && now - lastAt < REPEAT_AFTER_MS) return
        lastText = text
        lastAt = now
        onScanned(mapOf(TEXT to text))
    }

    private class QrAnalyzer(private val found: (String) -> Unit) : ImageAnalysis.Analyzer {
        private val reader = QRCodeReader()
        private var luminance = ByteArray(0)

        override fun analyze(image: ImageProxy) {
            image.use { frame ->
                val plane = frame.planes[0]
                val length = plane.buffer.remaining()
                if (luminance.size < length) luminance = ByteArray(length)
                plane.buffer.get(luminance, 0, length)
                val source = PlanarYUVLuminanceSource(luminance, plane.rowStride, frame.height, 0, 0, frame.width, frame.height, false)
                try {
                    found(reader.decode(BinaryBitmap(HybridBinarizer(source))).text)
                } catch (missed: ReaderException) {
                    reader.reset()
                }
            }
        }
    }

    companion object {
        private const val TEXT = "text"
        private const val REASON = "reason"
        private const val DENIED = "denied"
        private const val UNAVAILABLE = "unavailable"
        private const val REPEAT_AFTER_MS = 2000L
    }
}
