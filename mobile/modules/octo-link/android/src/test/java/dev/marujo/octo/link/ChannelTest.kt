package dev.marujo.octo.link

import java.math.BigInteger
import java.security.AlgorithmParameters
import java.security.GeneralSecurityException
import java.security.InvalidKeyException
import java.security.KeyFactory
import java.security.KeyPair
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPrivateKeySpec
import java.security.spec.ECPublicKeySpec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class ChannelTest {
    private fun bytes(hex: String): ByteArray = ByteArray(hex.length / 2) { hex.substring(it * 2, it * 2 + 2).toInt(16).toByte() }

    private fun phone(): Channel.Ephemeral {
        val params = AlgorithmParameters.getInstance("EC").apply { init(ECGenParameterSpec("secp256r1")) }.getParameterSpec(ECParameterSpec::class.java)
        val public = bytes(PHONE_PUBLIC)
        val point = ECPoint(BigInteger(1, public.copyOfRange(1, 33)), BigInteger(1, public.copyOfRange(33, 65)))
        val factory = KeyFactory.getInstance("EC")
        val pair = KeyPair(factory.generatePublic(ECPublicKeySpec(point, params)), factory.generatePrivate(ECPrivateKeySpec(BigInteger(1, bytes(PHONE_PRIVATE)), params)))
        return Channel.Ephemeral(pair)
    }

    @Test
    fun the_phone_key_goes_out_as_an_uncompressed_point() {
        assertArrayEquals(bytes(PHONE_PUBLIC), phone().publicKey)
    }

    @Test
    fun frames_match_what_the_web_crypto_of_the_phone_app_seals_and_opens() {
        val channel = phone().establish(bytes(BRIDGE_PUBLIC), SECRET)
        assertArrayEquals(bytes(UP_0), channel.seal("{\"type\":\"sync\"}".toByteArray()))
        assertArrayEquals(bytes(UP_1), channel.seal("{\"type\":\"close\"}".toByteArray()))
        assertEquals("{\"type\":\"welcome\"}", String(channel.open(bytes(DOWN_0))))
        assertEquals("state", String(channel.open(bytes(DOWN_1))))
    }

    @Test
    fun a_replayed_or_reordered_frame_is_refused() {
        val channel = phone().establish(bytes(BRIDGE_PUBLIC), SECRET)
        assertThrows(GeneralSecurityException::class.java) { channel.open(bytes(DOWN_1)) }
        val fresh = phone().establish(bytes(BRIDGE_PUBLIC), SECRET)
        fresh.open(bytes(DOWN_0))
        assertThrows(GeneralSecurityException::class.java) { fresh.open(bytes(DOWN_0)) }
    }

    @Test
    fun another_secret_cannot_read_the_computer() {
        val channel = phone().establish(bytes(BRIDGE_PUBLIC), ByteArray(Channel.SECRET_LEN) { 9 })
        assertThrows(GeneralSecurityException::class.java) { channel.open(bytes(DOWN_0)) }
    }

    @Test
    fun a_key_off_the_curve_or_a_short_secret_is_refused() {
        val off = bytes(BRIDGE_PUBLIC).also { it[64] = (it[64] + 1).toByte() }
        assertThrows(InvalidKeyException::class.java) { phone().establish(off, SECRET) }
        assertThrows(InvalidKeyException::class.java) { phone().establish(bytes(BRIDGE_PUBLIC), ByteArray(16)) }
        assertThrows(InvalidKeyException::class.java) { phone().establish(bytes(BRIDGE_PUBLIC).copyOf(33), SECRET) }
    }

    companion object {
        private val SECRET = ByteArray(Channel.SECRET_LEN) { 7 }
        private const val PHONE_PRIVATE = "b5c1f1f8f75f4063e3fd9476f39c4d4da18a641d8529bbe4cbe90a0e2e767cd0"
        private const val PHONE_PUBLIC = "046b5daeeed7cc76578d520c294bdbd0899ad97847cbabb8347206139ec39c824a6291b59fd296d5ba14bf33ec9992a7b442d892164b73cb2214967b1037da46ca"
        private const val BRIDGE_PUBLIC = "046fc81b50c30175e1fa03e7275a3304c1bd29a2b4a905767e1f0c46c87a95d686e175c90aa9b9d05c3fee75eafb9c392abba487572b270ccaa749b776b6e1091e"
        private const val UP_0 = "9e962d5a3e23c84411d56624ae2be770293d5f7056254fc4c9170587a269e5"
        private const val UP_1 = "c2f925caa8974f4cf99462643d32a6865e46818fe715da70ed88711cbae8b69e"
        private const val DOWN_0 = "964674689c6a41330e46573126b0aa41832d2f755444f91da35216957b7dd3d535de"
        private const val DOWN_1 = "90808524bbcddbda676c226f39eae18acce0dd7899"
    }
}
