import CryptoKit
import Foundation

enum ChannelError: Error {
  case secretLength
  case peerKey
  case frame
}

enum Base64Url {
  static func encode(_ data: Data) -> String {
    data.base64EncodedString()
      .replacingOccurrences(of: "+", with: "-")
      .replacingOccurrences(of: "/", with: "_")
      .replacingOccurrences(of: "=", with: "")
  }

  static func decode(_ text: String) -> Data? {
    var padded = text
      .replacingOccurrences(of: "-", with: "+")
      .replacingOccurrences(of: "_", with: "/")
    while padded.count % 4 != 0 {
      padded.append("=")
    }
    return Data(base64Encoded: padded)
  }
}

final class Channel {
  static let version = 2
  static let secretLength = 32
  static let publicKeyLength = 65
  private static let keyLength = 32
  private static let tagLength = 16
  private static let noncePadding = 4
  private static let phoneToBridge = Data("orkai-pwa/2 phone->bridge".utf8)
  private static let bridgeToPhone = Data("orkai-pwa/2 bridge->phone".utf8)

  private let sealKey: SymmetricKey
  private let openKey: SymmetricKey
  private var sent: UInt64 = 0
  private var received: UInt64 = 0

  init(own: P256.KeyAgreement.PrivateKey, peer: Data, secret: Data) throws {
    guard secret.count == Channel.secretLength else {
      throw ChannelError.secretLength
    }
    guard peer.count == Channel.publicKeyLength else {
      throw ChannelError.peerKey
    }
    let theirs = try P256.KeyAgreement.PublicKey(x963Representation: peer)
    let shared = try own.sharedSecretFromKeyAgreement(with: theirs)
    let mine = own.publicKey.x963Representation
    sealKey = shared.hkdfDerivedSymmetricKey(
      using: SHA256.self,
      salt: secret,
      sharedInfo: Channel.phoneToBridge + mine + peer,
      outputByteCount: Channel.keyLength
    )
    openKey = shared.hkdfDerivedSymmetricKey(
      using: SHA256.self,
      salt: secret,
      sharedInfo: Channel.bridgeToPhone + mine + peer,
      outputByteCount: Channel.keyLength
    )
  }

  func seal(_ plain: Data) throws -> Data {
    let nonce = try Channel.nonce(sent)
    sent += 1
    let box = try AES.GCM.seal(plain, using: sealKey, nonce: nonce)
    return box.ciphertext + box.tag
  }

  func open(_ frame: Data) throws -> Data {
    guard frame.count >= Channel.tagLength else {
      throw ChannelError.frame
    }
    let nonce = try Channel.nonce(received)
    received += 1
    let box = try AES.GCM.SealedBox(
      nonce: nonce,
      ciphertext: frame.dropLast(Channel.tagLength),
      tag: frame.suffix(Channel.tagLength)
    )
    return try AES.GCM.open(box, using: openKey)
  }

  private static func nonce(_ counter: UInt64) throws -> AES.GCM.Nonce {
    var bytes = Data(count: noncePadding)
    withUnsafeBytes(of: counter.bigEndian) { bytes.append(contentsOf: $0) }
    return try AES.GCM.Nonce(data: bytes)
  }
}
