import CryptoKit
import XCTest

@testable import OctoLink

final class ChannelTests: XCTestCase {
  private static let secret = Data(repeating: 7, count: Channel.secretLength)
  private static let phonePrivate = "b5c1f1f8f75f4063e3fd9476f39c4d4da18a641d8529bbe4cbe90a0e2e767cd0"
  private static let phonePublic = "046b5daeeed7cc76578d520c294bdbd0899ad97847cbabb8347206139ec39c824a6291b59fd296d5ba14bf33ec9992a7b442d892164b73cb2214967b1037da46ca"
  private static let bridgePublic = "046fc81b50c30175e1fa03e7275a3304c1bd29a2b4a905767e1f0c46c87a95d686e175c90aa9b9d05c3fee75eafb9c392abba487572b270ccaa749b776b6e1091e"
  private static let up0 = "9e962d5a3e23c84411d56624ae2be770293d5f7056254fc4c9170587a269e5"
  private static let up1 = "c2f925caa8974f4cf99462643d32a6865e46818fe715da70ed88711cbae8b69e"
  private static let down0 = "964674689c6a41330e46573126b0aa41832d2f755444f91da35216957b7dd3d535de"
  private static let down1 = "90808524bbcddbda676c226f39eae18acce0dd7899"

  private func bytes(_ hex: String) -> Data {
    var data = Data()
    var index = hex.startIndex
    while index < hex.endIndex {
      let next = hex.index(index, offsetBy: 2)
      data.append(UInt8(hex[index..<next], radix: 16)!)
      index = next
    }
    return data
  }

  private func phone() throws -> P256.KeyAgreement.PrivateKey {
    try P256.KeyAgreement.PrivateKey(rawRepresentation: bytes(ChannelTests.phonePrivate))
  }

  private func channel(secret: Data = ChannelTests.secret) throws -> Channel {
    try Channel(own: phone(), peer: bytes(ChannelTests.bridgePublic), secret: secret)
  }

  func test_the_phone_key_goes_out_as_an_uncompressed_point() throws {
    XCTAssertEqual(try phone().publicKey.x963Representation, bytes(ChannelTests.phonePublic))
  }

  func test_frames_match_what_the_web_crypto_of_the_phone_app_seals_and_opens() throws {
    let link = try channel()
    XCTAssertEqual(try link.seal(Data(#"{"type":"sync"}"#.utf8)), bytes(ChannelTests.up0))
    XCTAssertEqual(try link.seal(Data(#"{"type":"close"}"#.utf8)), bytes(ChannelTests.up1))
    XCTAssertEqual(String(decoding: try link.open(bytes(ChannelTests.down0)), as: UTF8.self), #"{"type":"welcome"}"#)
    XCTAssertEqual(String(decoding: try link.open(bytes(ChannelTests.down1)), as: UTF8.self), "state")
  }

  func test_a_replayed_or_reordered_frame_is_refused() throws {
    XCTAssertThrowsError(try channel().open(bytes(ChannelTests.down1)))
    let fresh = try channel()
    _ = try fresh.open(bytes(ChannelTests.down0))
    XCTAssertThrowsError(try fresh.open(bytes(ChannelTests.down0)))
  }

  func test_another_secret_cannot_read_the_computer() throws {
    let other = try channel(secret: Data(repeating: 9, count: Channel.secretLength))
    XCTAssertThrowsError(try other.open(bytes(ChannelTests.down0)))
  }

  func test_a_key_off_the_curve_or_a_short_secret_is_refused() throws {
    var off = bytes(ChannelTests.bridgePublic)
    off[64] &+= 1
    XCTAssertThrowsError(try Channel(own: phone(), peer: off, secret: ChannelTests.secret))
    XCTAssertThrowsError(try channel(secret: Data(count: 16)))
    XCTAssertThrowsError(try Channel(own: phone(), peer: bytes(ChannelTests.bridgePublic).prefix(33), secret: ChannelTests.secret))
  }

  func test_the_pairing_key_reads_back_from_its_link_form() {
    let key = Base64Url.decode("Qm9vdHN0cmFwcGluZyB0aGUgb3JrYWkgcGFpcmluZyE")
    XCTAssertEqual(key, Data("Bootstrapping the orkai pairing!".utf8))
    XCTAssertEqual(Base64Url.encode(key!), "Qm9vdHN0cmFwcGluZyB0aGUgb3JrYWkgcGFpcmluZyE")
  }
}
