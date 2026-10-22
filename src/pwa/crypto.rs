use std::fmt;
use std::io;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::agreement::{self, ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey};
use ring::hkdf;
use ring::rand::SystemRandom;

use crate::util::random_bytes;

pub const PUBLIC_KEY_LEN: usize = 65;
pub const SECRET_LEN: usize = 32;
pub const TAG_LEN: usize = 16;
pub const PHONE_TO_BRIDGE: &[u8] = b"orkai-pwa/2 phone->bridge";
pub const BRIDGE_TO_PHONE: &[u8] = b"orkai-pwa/2 bridge->phone";
const SHARED_LEN: usize = 32;
const KEY_LEN: usize = 32;
const COUNTER_AT: usize = NONCE_LEN - 8;

pub type Secret = [u8; SECRET_LEN];
pub type PublicKey = [u8; PUBLIC_KEY_LEN];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    KeyAgreement,
    Decrypt,
    Exhausted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::KeyAgreement => write!(f, "the key agreement failed"),
            Error::Decrypt => write!(f, "a frame did not decrypt"),
            Error::Exhausted => write!(f, "the channel used every nonce"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Bridge,
    Phone,
}

pub fn secret() -> io::Result<Secret> {
    random_bytes::<SECRET_LEN>()
}

pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text.trim()).ok()
}

pub fn decode_secret(text: &str) -> Option<Secret> {
    decode(text)?.try_into().ok()
}

pub fn decode_public_key(text: &str) -> Option<PublicKey> {
    decode(text)?.try_into().ok()
}

pub struct Ephemeral {
    private: EphemeralPrivateKey,
    public: PublicKey,
}

impl Ephemeral {
    pub fn generate() -> Result<Self, Error> {
        let private =
            EphemeralPrivateKey::generate(&ECDH_P256, &SystemRandom::new()).map_err(|_| Error::KeyAgreement)?;
        let computed = private.compute_public_key().map_err(|_| Error::KeyAgreement)?;
        let public = computed.as_ref().try_into().map_err(|_| Error::KeyAgreement)?;
        Ok(Self { private, public })
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    fn agree(self, peer: &PublicKey) -> Result<[u8; SHARED_LEN], Error> {
        let peer = UnparsedPublicKey::new(&ECDH_P256, peer);
        agreement::agree_ephemeral(self.private, &peer, |shared| <[u8; SHARED_LEN]>::try_from(shared))
            .map_err(|_| Error::KeyAgreement)?
            .map_err(|_| Error::KeyAgreement)
    }
}

struct Direction {
    key: LessSafeKey,
    counter: u64,
}

impl Direction {
    fn new(key: &[u8; KEY_LEN]) -> Result<Self, Error> {
        let unbound = UnboundKey::new(&AES_256_GCM, key).map_err(|_| Error::KeyAgreement)?;
        Ok(Self { key: LessSafeKey::new(unbound), counter: 0 })
    }

    fn next_nonce(&mut self) -> Result<Nonce, Error> {
        if self.counter == u64::MAX {
            return Err(Error::Exhausted);
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce[COUNTER_AT..].copy_from_slice(&self.counter.to_be_bytes());
        self.counter += 1;
        Ok(Nonce::assume_unique_for_key(nonce))
    }
}

pub struct Channel {
    sealer: Direction,
    opener: Direction,
}

impl Channel {
    pub fn establish(side: Side, own: Ephemeral, peer: &PublicKey, secret: &Secret) -> Result<Self, Error> {
        let own_public = *own.public_key();
        let shared = own.agree(peer)?;
        let (phone, bridge) = match side {
            Side::Bridge => (peer, &own_public),
            Side::Phone => (&own_public, peer),
        };
        let up = derive(secret, &shared, PHONE_TO_BRIDGE, phone, bridge)?;
        let down = derive(secret, &shared, BRIDGE_TO_PHONE, phone, bridge)?;
        let (seal, open) = match side {
            Side::Bridge => (down, up),
            Side::Phone => (up, down),
        };
        Ok(Self { sealer: Direction::new(&seal)?, opener: Direction::new(&open)? })
    }

    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, Error> {
        let nonce = self.sealer.next_nonce()?;
        let mut out = Vec::with_capacity(plaintext.len() + TAG_LEN);
        out.extend_from_slice(plaintext);
        self.sealer.key.seal_in_place_append_tag(nonce, Aad::empty(), &mut out).map_err(|_| Error::Decrypt)?;
        Ok(out)
    }

    pub fn open(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, Error> {
        let nonce = self.opener.next_nonce()?;
        let mut buffer = ciphertext.to_vec();
        let plain = self.opener.key.open_in_place(nonce, Aad::empty(), &mut buffer).map_err(|_| Error::Decrypt)?;
        let len = plain.len();
        buffer.truncate(len);
        Ok(buffer)
    }
}

fn derive(
    secret: &Secret,
    shared: &[u8; SHARED_LEN],
    label: &[u8],
    phone: &PublicKey,
    bridge: &PublicKey,
) -> Result<[u8; KEY_LEN], Error> {
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, secret).extract(shared);
    let info = [label, phone.as_slice(), bridge.as_slice()];
    let okm = prk.expand(&info, hkdf::HKDF_SHA256).map_err(|_| Error::KeyAgreement)?;
    let mut key = [0u8; KEY_LEN];
    okm.fill(&mut key).map_err(|_| Error::KeyAgreement)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(phone_secret: &Secret, bridge_secret: &Secret) -> (Channel, Channel) {
        let phone = Ephemeral::generate().unwrap();
        let bridge = Ephemeral::generate().unwrap();
        let (phone_public, bridge_public) = (*phone.public_key(), *bridge.public_key());
        let phone = Channel::establish(Side::Phone, phone, &bridge_public, phone_secret).unwrap();
        let bridge = Channel::establish(Side::Bridge, bridge, &phone_public, bridge_secret).unwrap();
        (phone, bridge)
    }

    #[test]
    fn both_directions_round_trip_in_order() {
        let secret = secret().unwrap();
        let (mut phone, mut bridge) = pair(&secret, &secret);
        for text in ["one", "two", "three"] {
            let frame = phone.seal(text.as_bytes()).unwrap();
            assert_ne!(frame.as_slice(), text.as_bytes());
            assert_eq!(bridge.open(&frame).unwrap(), text.as_bytes());
        }
        let frame = bridge.seal(b"state").unwrap();
        assert_eq!(phone.open(&frame).unwrap(), b"state");
    }

    #[test]
    fn a_phone_without_the_secret_cannot_talk_to_the_bridge() {
        let (mut phone, mut bridge) = pair(&secret().unwrap(), &secret().unwrap());
        assert_eq!(bridge.open(&phone.seal(b"keys").unwrap()), Err(Error::Decrypt));
        assert_eq!(phone.open(&bridge.seal(b"state").unwrap()), Err(Error::Decrypt));
    }

    #[test]
    fn a_replayed_frame_is_rejected() {
        let secret = secret().unwrap();
        let (mut phone, mut bridge) = pair(&secret, &secret);
        let frame = phone.seal(b"rm -rf\r").unwrap();
        assert!(bridge.open(&frame).is_ok());
        assert_eq!(bridge.open(&frame), Err(Error::Decrypt));
    }

    #[test]
    fn a_reordered_or_dropped_frame_is_rejected() {
        let secret = secret().unwrap();
        let (mut phone, mut bridge) = pair(&secret, &secret);
        let _first = phone.seal(b"first").unwrap();
        let second = phone.seal(b"second").unwrap();
        assert_eq!(bridge.open(&second), Err(Error::Decrypt));
    }

    #[test]
    fn a_frame_is_never_reflected_back_to_its_sender() {
        let secret = secret().unwrap();
        let (mut phone, _bridge) = pair(&secret, &secret);
        let frame = phone.seal(b"keys").unwrap();
        assert_eq!(phone.open(&frame), Err(Error::Decrypt));
    }

    #[test]
    fn a_tampered_frame_is_rejected() {
        let secret = secret().unwrap();
        let (mut phone, mut bridge) = pair(&secret, &secret);
        let mut frame = phone.seal(b"hello").unwrap();
        frame[0] ^= 0x01;
        assert_eq!(bridge.open(&frame), Err(Error::Decrypt));
    }

    #[test]
    fn the_nonce_is_the_big_endian_counter() {
        let mut direction = Direction::new(&[7u8; KEY_LEN]).unwrap();
        direction.counter = 0x0102_0304_0506_0708;
        let nonce = direction.next_nonce().unwrap();
        assert_eq!(nonce.as_ref(), &[0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
        direction.counter = u64::MAX;
        assert!(matches!(direction.next_nonce(), Err(Error::Exhausted)));
    }

    #[test]
    fn keys_and_secrets_survive_the_text_encoding() {
        let secret = secret().unwrap();
        let text = encode(&secret);
        assert!(!text.contains(['=', '+', '/']));
        assert_eq!(decode_secret(&text), Some(secret));
        assert_eq!(decode_secret("short"), None);
        let key = Ephemeral::generate().unwrap();
        assert_eq!(decode_public_key(&encode(key.public_key())), Some(*key.public_key()));
        assert_eq!(decode_public_key(&encode(&secret)), None);
    }
}
