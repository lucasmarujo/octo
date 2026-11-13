package dev.marujo.octo.link

import android.util.Base64
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import okhttp3.OkHttpClient

class OctoLinkModule : Module(), Session.Events {
    private val sessions = ConcurrentHashMap<String, Session>()
    private val client: OkHttpClient by lazy {
        OkHttpClient.Builder()
            .connectTimeout(CONNECT_SECONDS, TimeUnit.SECONDS)
            .pingInterval(PING_SECONDS, TimeUnit.SECONDS)
            .build()
    }

    override fun definition() = ModuleDefinition {
        Name(NAME)

        Events(READY, MESSAGE, CLOSED)

        Function("open") { id: String, url: String, key: String, agent: String ->
            open(id, url, key, agent)
        }

        Function("send") { id: String, text: String ->
            sessions[id]?.send(text)
        }

        Function("queued") { id: String ->
            (sessions[id]?.queued() ?: 0L).toDouble()
        }

        Function("close") { id: String ->
            sessions.remove(id)?.close()
        }

        OnDestroy {
            for (id in sessions.keys.toList()) sessions.remove(id)?.close()
        }
    }

    override fun ready(id: String) {
        sendEvent(READY, mapOf(ID to id))
    }

    override fun message(id: String, text: String) {
        sendEvent(MESSAGE, mapOf(ID to id, TEXT to text))
    }

    override fun closed(id: String, code: Int, reason: String) {
        sessions.remove(id)
        sendEvent(CLOSED, mapOf(ID to id, CODE to code, REASON to reason))
    }

    private fun open(id: String, url: String, key: String, agent: String) {
        val secret = try {
            Base64.decode(key, Session.BASE64_URL)
        } catch (error: IllegalArgumentException) {
            ByteArray(0)
        }
        if (secret.size != Channel.SECRET_LEN) {
            closed(id, Session.ABNORMAL, "the pairing key is not valid")
            return
        }
        val session = Session(id, url, secret, agent, this)
        sessions.put(id, session)?.close()
        session.start(client)
    }

    companion object {
        private const val NAME = "OctoLink"
        private const val READY = "onReady"
        private const val MESSAGE = "onMessage"
        private const val CLOSED = "onClosed"
        private const val CONNECT_SECONDS = 10L
        private const val PING_SECONDS = 20L
        private const val ID = "id"
        private const val TEXT = "text"
        private const val CODE = "code"
        private const val REASON = "reason"
    }
}
