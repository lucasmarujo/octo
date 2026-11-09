package dev.marujo.octo.link

import java.math.BigInteger
import java.nio.ByteBuffer
import java.security.GeneralSecurityException
import java.security.InvalidKeyException
import java.security.KeyFactory
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.interfaces.ECPublicKey
import java.security.spec.ECFieldFp
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec
import javax.crypto.Cipher
import javax.crypto.KeyAgreement
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

class Channel private constructor(sealKey: ByteArray, openKey: ByteArray) {
    private val sealer = Direction(sealKey, Cipher.ENCRYPT_MODE)
    private val opener = Direction(openKey, Cipher.DECRYPT_MODE)

    @Throws(GeneralSecurityException::class)
    fun seal(plain: ByteArray): ByteArray = sealer.next(plain)

    @Throws(GeneralSecurityException::class)
    fun open(frame: ByteArray): ByteArray = opener.next(frame)

    private class Direction(key: ByteArray, private val mode: Int) {
        private val key = SecretKeySpec(key, CIPHER)
        private var counter = 0L

        @Synchronized
        fun next(input: ByteArray): ByteArray {
            val nonce = ByteBuffer.allocate(NONCE_LEN).putInt(0).putLong(counter).array()
            counter += 1
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(mode, key, GCMParameterSpec(TAG_BITS, nonce))
            return cipher.doFinal(input)
        }
    }

    class Ephemeral(private val pair: KeyPair) {
        private val params: ECParameterSpec = (pair.public as ECPublicKey).params
        val publicKey: ByteArray = encode((pair.public as ECPublicKey).w)

        @Throws(GeneralSecurityException::class)
        fun establish(peer: ByteArray, secret: ByteArray): Channel {
            if (secret.size != SECRET_LEN) throw InvalidKeyException("the pairing secret has the wrong length")
            val agreement = KeyAgreement.getInstance(AGREEMENT)
            agreement.init(pair.private)
            agreement.doPhase(KeyFactory.getInstance(CURVE_FAMILY).generatePublic(ECPublicKeySpec(decode(peer), params)), true)
            val shared = agreement.generateSecret()
            val up = derive(secret, shared, PHONE_TO_BRIDGE, publicKey, peer)
            val down = derive(secret, shared, BRIDGE_TO_PHONE, publicKey, peer)
            return Channel(up, down)
        }

        private fun decode(point: ByteArray): ECPoint {
            if (point.size != PUBLIC_KEY_LEN || point[0] != UNCOMPRESSED) throw InvalidKeyException("the peer key is not an uncompressed P-256 point")
            val x = BigInteger(1, point.copyOfRange(1, 1 + FIELD_LEN))
            val y = BigInteger(1, point.copyOfRange(1 + FIELD_LEN, PUBLIC_KEY_LEN))
            val curve = params.curve
            val p = (curve.field as ECFieldFp).p
            val right = x.pow(3).add(curve.a.multiply(x)).add(curve.b).mod(p)
            if (x >= p || y >= p || y.multiply(y).mod(p) != right) throw InvalidKeyException("the peer key is not on the curve")
            return ECPoint(x, y)
        }

        companion object {
            @Throws(GeneralSecurityException::class)
            fun generate(): Ephemeral {
                val generator = KeyPairGenerator.getInstance(CURVE_FAMILY)
                generator.initialize(ECGenParameterSpec(CURVE))
                return Ephemeral(generator.generateKeyPair())
            }

            private fun encode(point: ECPoint): ByteArray {
                val out = ByteArray(PUBLIC_KEY_LEN)
                out[0] = UNCOMPRESSED
                fixed(point.affineX).copyInto(out, 1)
                fixed(point.affineY).copyInto(out, 1 + FIELD_LEN)
                return out
            }

            private fun fixed(value: BigInteger): ByteArray {
                val bytes = value.toByteArray()
                val out = ByteArray(FIELD_LEN)
                val length = minOf(bytes.size, FIELD_LEN)
                bytes.copyInto(out, FIELD_LEN - length, bytes.size - length, bytes.size)
                return out
            }

            private fun derive(secret: ByteArray, shared: ByteArray, label: ByteArray, phone: ByteArray, bridge: ByteArray): ByteArray {
                val extracted = hmac(secret, shared)
                return hmac(extracted, label + phone + bridge + FIRST_BLOCK)
            }

            private fun hmac(key: ByteArray, data: ByteArray): ByteArray {
                val mac = Mac.getInstance(HMAC)
                mac.init(SecretKeySpec(key, HMAC))
                return mac.doFinal(data)
            }
        }
    }

    companion object {
        const val VERSION = 2
        const val SECRET_LEN = 32
        const val PUBLIC_KEY_LEN = 65
        private const val FIELD_LEN = 32
        private const val NONCE_LEN = 12
        private const val TAG_BITS = 128
        private const val UNCOMPRESSED: Byte = 4
        private const val CURVE_FAMILY = "EC"
        private const val CURVE = "secp256r1"
        private const val AGREEMENT = "ECDH"
        private const val HMAC = "HmacSHA256"
        private const val CIPHER = "AES"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private val FIRST_BLOCK = byteArrayOf(1)
        private val PHONE_TO_BRIDGE = "orkai-pwa/2 phone->bridge".toByteArray(Charsets.US_ASCII)
        private val BRIDGE_TO_PHONE = "orkai-pwa/2 bridge->phone".toByteArray(Charsets.US_ASCII)
    }
}
