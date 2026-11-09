package dev.marujo.octo.link

import android.util.Base64
import java.security.GeneralSecurityException
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import okio.ByteString.Companion.toByteString
import org.json.JSONException
import org.json.JSONObject

internal class Session(
    private val id: String,
    private val url: String,
    private val secret: ByteArray,
    private val agent: String,
    private val events: Events,
) : WebSocketListener() {
    interface Events {
        fun ready(id: String)
        fun message(id: String, text: String)
        fun closed(id: String, code: Int, reason: String)
    }

    private val lock = Any()
    private var own: Channel.Ephemeral? = null
    private var channel: Channel? = null
    private var socket: WebSocket? = null
    private var finished = false

    fun start(client: OkHttpClient) {
        try {
            val request = Request.Builder().url(url).header(USER_AGENT, agent).build()
            synchronized(lock) {
                own = Channel.Ephemeral.generate()
                socket = client.newWebSocket(request, this)
            }
        } catch (error: GeneralSecurityException) {
            finish(ABNORMAL, error.message.orEmpty())
        } catch (error: IllegalArgumentException) {
            finish(ABNORMAL, error.message.orEmpty())
        }
    }

    fun send(text: String) {
        synchronized(lock) {
            val open = channel ?: return
            if (finished) return
            try {
                socket?.send(open.seal(text.toByteArray(Charsets.UTF_8)).toByteString())
            } catch (error: GeneralSecurityException) {
                fail(error)
            }
        }
    }

    fun close() {
        synchronized(lock) {
            finished = true
            socket?.close(NORMAL, null)
        }
    }

    override fun onOpen(webSocket: WebSocket, response: Response) {
        val key = synchronized(lock) { own?.publicKey } ?: return
        val hello = JSONObject().put(TYPE, HELLO).put(VERSION, Channel.VERSION).put(KEY, encode(key))
        webSocket.send(hello.toString())
    }

    override fun onMessage(webSocket: WebSocket, text: String) {
        val peer = greeting(text)
        if (peer == null) {
            webSocket.close(NORMAL, null)
            finish(OUTDATED, "")
            return
        }
        try {
            synchronized(lock) {
                if (channel != null || finished) return
                val established = (own ?: return).establish(peer, secret)
                own = null
                channel = established
                webSocket.send(established.seal(SYNC).toByteString())
            }
            events.ready(id)
        } catch (error: GeneralSecurityException) {
            fail(error)
        }
    }

    override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
        val open = synchronized(lock) { channel }
        if (open == null) {
            fail(GeneralSecurityException("a frame arrived before the greeting"))
            return
        }
        val plain = try {
            open.open(bytes.toByteArray())
        } catch (error: GeneralSecurityException) {
            fail(error)
            return
        }
        if (!synchronized(lock) { finished }) events.message(id, String(plain, Charsets.UTF_8))
    }

    override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
        webSocket.close(NORMAL, null)
        finish(code, reason)
    }

    override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
        finish(code, reason)
    }

    override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
        finish(ABNORMAL, t.message.orEmpty())
    }

    private fun greeting(text: String): ByteArray? {
        return try {
            val greeting = JSONObject(text)
            if (greeting.optString(TYPE) != HELLO || greeting.optInt(VERSION) != Channel.VERSION) return null
            val key = Base64.decode(greeting.getString(KEY), BASE64_URL)
            if (key.size == Channel.PUBLIC_KEY_LEN) key else null
        } catch (error: JSONException) {
            null
        } catch (error: IllegalArgumentException) {
            null
        }
    }

    private fun fail(error: Exception) {
        synchronized(lock) { socket }?.close(NORMAL, null)
        finish(ABNORMAL, error.message.orEmpty())
    }

    private fun finish(code: Int, reason: String) {
        synchronized(lock) {
            if (finished) return
            finished = true
        }
        events.closed(id, code, reason)
    }

    companion object {
        const val ABNORMAL = 1006
        const val OUTDATED = 4002
        const val BASE64_URL = Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP
        private const val NORMAL = 1000
        private const val USER_AGENT = "User-Agent"
        private const val TYPE = "type"
        private const val HELLO = "hello"
        private const val VERSION = "version"
        private const val KEY = "key"
        private val SYNC = "{\"type\":\"sync\"}".toByteArray(Charsets.UTF_8)

        private fun encode(bytes: ByteArray): String = Base64.encodeToString(bytes, BASE64_URL)
    }
}
