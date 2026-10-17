//! The envelope: the outer binary container every carrier transports.
//!
//! Layout (all integers big-endian), see `docs/SPEC.md` §Envelope:
//!
//! ```text
//! offset  size  field
//! 0       4     magic = "POLY"
//! 4       1     version = 1
//! 5       1     algorithm
//! 6       1     flags (bit 0 = SESSION, others reserved = 0)
//! 7       8     contact_id
//! 15      4     payload_len (u32)
//! 19      N     payload
//! 19+N    4     CRC-32 (IEEE) of bytes [0, 19+N)
//! ```

use sha2::{Digest, Sha256};

use crate::PolyError;
use crate::types::{AAD_LEN, Algorithm, CONTACT_ID_LEN, ContactId};

/// ASCII "POLY". Random data matches 4 bytes only 1 in 2^32 times,
/// so carrier auto-detect almost never gives false positives.
pub const MAGIC: [u8; 4] = *b"POLY";
/// The only envelope version this build reads and writes.
pub const VERSION: u8 = 1;

/// Flag bit 0: SESSION mode (no KEM block; uses a saved session key).
/// Reserved for future work; the MVP always writes 0.
pub const FLAG_SESSION: u8 = 0b0000_0001;
/// All flag bits this version defines. Any other bit set is an error.
pub const KNOWN_FLAGS: u8 = FLAG_SESSION;

/// Byte offset of each header field.
pub const OFFSET_MAGIC: usize = 0;
pub const OFFSET_VERSION: usize = 4;
pub const OFFSET_ALGORITHM: usize = 5;
pub const OFFSET_FLAGS: usize = 6;
pub const OFFSET_CONTACT_ID: usize = 7;
pub const OFFSET_PAYLOAD_LEN: usize = OFFSET_CONTACT_ID + CONTACT_ID_LEN; // 15
pub const OFFSET_PAYLOAD: usize = OFFSET_PAYLOAD_LEN + PAYLOAD_LEN_SIZE; // 19

/// Size of the u32 payload_len field. u32 because images can carry
/// about 98 KB, more than u16's maximum of 65,535.
pub const PAYLOAD_LEN_SIZE: usize = 4;
/// Size of the trailing CRC-32.
pub const CRC_SIZE: usize = 4;
/// Header length (magic through payload_len).
pub const HEADER_LEN: usize = OFFSET_PAYLOAD;
/// Total bytes an envelope adds around its payload: 19 + 4 = 23.
pub const ENVELOPE_OVERHEAD: usize = HEADER_LEN + CRC_SIZE;

// The AAD is exactly the header up to (not including) payload_len.
const _: () = assert!(OFFSET_PAYLOAD_LEN == AAD_LEN);

/// A parsed envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub version: u8,
    pub algorithm: Algorithm,
    pub flags: u8,
    pub contact_id: ContactId,
    pub payload: Vec<u8>,
}

impl Envelope {
    /// A version-1 envelope.
    pub fn new(algorithm: Algorithm, flags: u8, contact_id: ContactId, payload: Vec<u8>) -> Self {
        Envelope {
            version: VERSION,
            algorithm,
            flags,
            contact_id,
            payload,
        }
    }

    /// Total serialized length for a payload of `payload_len` bytes.
    pub fn encoded_len(payload_len: usize) -> usize {
        ENVELOPE_OVERHEAD + payload_len
    }

    /// Header bytes 0..15 (magic through contact_id): the AES-GCM associated data.
    pub fn aad(&self) -> [u8; AAD_LEN] {
        header_aad(self.algorithm, self.flags, &self.contact_id)
    }

    /// Whether the SESSION flag is set.
    pub fn is_session(&self) -> bool {
        self.flags & FLAG_SESSION != 0
    }

    /// Serialize. Fails only if the payload exceeds u32::MAX bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, PolyError> {
        let payload_len = u32::try_from(self.payload.len())
            .map_err(|_| PolyError::PayloadTooLarge(self.payload.len()))?;
        let mut out = Vec::with_capacity(Self::encoded_len(self.payload.len()));
        out.extend_from_slice(&MAGIC);
        out.push(self.version);
        out.push(self.algorithm.id());
        out.push(self.flags);
        out.extend_from_slice(&self.contact_id);
        out.extend_from_slice(&payload_len.to_be_bytes());
        out.extend_from_slice(&self.payload);
        let crc = crc32fast::hash(&out);
        out.extend_from_slice(&crc.to_be_bytes());
        Ok(out)
    }

    /// Parse an envelope. Trailing bytes after the CRC are ignored (carriers may pad).
    /// Never panics.
    ///
    /// Check order: magic, length, CRC, then version / algorithm / flags. The
    /// CRC is checked before the field values so a damaged file reports
    /// [`PolyError::ChecksumMismatch`] ("compressed in transit") rather than a
    /// confusing "unknown algorithm".
    pub fn from_bytes(bytes: &[u8]) -> Result<Envelope, PolyError> {
        let magic = bytes
            .get(OFFSET_MAGIC..OFFSET_MAGIC + MAGIC.len())
            .ok_or(PolyError::Truncated)?;
        if magic != MAGIC {
            return Err(PolyError::BadMagic);
        }
        let header = bytes.get(..HEADER_LEN).ok_or(PolyError::Truncated)?;
        let mut len_bytes = [0u8; PAYLOAD_LEN_SIZE];
        len_bytes.copy_from_slice(&header[OFFSET_PAYLOAD_LEN..OFFSET_PAYLOAD]);
        let payload_len =
            usize::try_from(u32::from_be_bytes(len_bytes)).map_err(|_| PolyError::Truncated)?;
        let crc_start = HEADER_LEN
            .checked_add(payload_len)
            .ok_or(PolyError::Truncated)?;
        let crc_end = crc_start
            .checked_add(CRC_SIZE)
            .ok_or(PolyError::Truncated)?;
        let crc_bytes = bytes.get(crc_start..crc_end).ok_or(PolyError::Truncated)?;
        let mut crc_arr = [0u8; CRC_SIZE];
        crc_arr.copy_from_slice(crc_bytes);
        if crc32fast::hash(&bytes[..crc_start]) != u32::from_be_bytes(crc_arr) {
            return Err(PolyError::ChecksumMismatch);
        }

        let version = header[OFFSET_VERSION];
        if version != VERSION {
            return Err(PolyError::UnsupportedVersion(version));
        }
        let algorithm = Algorithm::from_id(header[OFFSET_ALGORITHM])?;
        let flags = header[OFFSET_FLAGS];
        if flags & !KNOWN_FLAGS != 0 {
            return Err(PolyError::ReservedFlags(flags));
        }
        let mut contact_id = [0u8; CONTACT_ID_LEN];
        contact_id.copy_from_slice(&header[OFFSET_CONTACT_ID..OFFSET_PAYLOAD_LEN]);

        Ok(Envelope {
            version,
            algorithm,
            flags,
            contact_id,
            payload: bytes[HEADER_LEN..crc_start].to_vec(),
        })
    }
}

/// Build header bytes 0..15 (the AAD) without building a whole envelope.
pub fn header_aad(algorithm: Algorithm, flags: u8, contact_id: &ContactId) -> [u8; AAD_LEN] {
    let mut aad = [0u8; AAD_LEN];
    aad[OFFSET_MAGIC..OFFSET_VERSION].copy_from_slice(&MAGIC);
    aad[OFFSET_VERSION] = VERSION;
    aad[OFFSET_ALGORITHM] = algorithm.id();
    aad[OFFSET_FLAGS] = flags;
    aad[OFFSET_CONTACT_ID..OFFSET_PAYLOAD_LEN].copy_from_slice(contact_id);
    aad
}

/// contact_id = first 8 bytes of SHA-256(recipient public key).
pub fn contact_id(public_key: &[u8]) -> ContactId {
    let digest = Sha256::digest(public_key);
    let mut id = [0u8; CONTACT_ID_LEN];
    id.copy_from_slice(&digest[..CONTACT_ID_LEN]);
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const CID: ContactId = [1, 2, 3, 4, 5, 6, 7, 8];

    fn sample() -> Envelope {
        Envelope::new(Algorithm::Fake, 0, CID, b"hello payload".to_vec())
    }

    /// Recompute the CRC after editing header bytes, to test field checks.
    fn fix_crc(bytes: &mut [u8]) {
        let n = bytes.len() - CRC_SIZE;
        let crc = crc32fast::hash(&bytes[..n]);
        bytes[n..].copy_from_slice(&crc.to_be_bytes());
    }

    #[test]
    fn roundtrip() {
        let env = sample();
        let bytes = env.to_bytes().unwrap();
        assert_eq!(bytes.len(), ENVELOPE_OVERHEAD + env.payload.len());
        assert_eq!(&bytes[..4], b"POLY");
        assert_eq!(Envelope::from_bytes(&bytes).unwrap(), env);
    }

    #[test]
    fn overhead_is_23() {
        assert_eq!(ENVELOPE_OVERHEAD, 23);
        assert_eq!(HEADER_LEN, 19);
    }

    #[test]
    fn empty_payload() {
        let env = Envelope::new(Algorithm::Fake, 0, CID, vec![]);
        let bytes = env.to_bytes().unwrap();
        assert_eq!(bytes.len(), 23);
        assert_eq!(Envelope::from_bytes(&bytes).unwrap(), env);
    }

    #[test]
    fn layout_is_big_endian() {
        let mut env = sample();
        env.payload = vec![0xAA; 0x0102];
        let b = env.to_bytes().unwrap();
        assert_eq!(b[4], 1);
        assert_eq!(b[5], 0xFF);
        assert_eq!(b[6], 0);
        assert_eq!(&b[7..15], &CID);
        assert_eq!(&b[15..19], &[0, 0, 0x01, 0x02]);
        let crc = crc32fast::hash(&b[..b.len() - 4]);
        assert_eq!(&b[b.len() - 4..], &crc.to_be_bytes());
    }

    #[test]
    fn aad_is_header_prefix() {
        let env = sample();
        let bytes = env.to_bytes().unwrap();
        assert_eq!(env.aad(), bytes[..AAD_LEN]);
    }

    #[test]
    fn ignores_trailing_bytes() {
        let env = sample();
        let mut bytes = env.to_bytes().unwrap();
        bytes.extend_from_slice(&[0u8; 100]);
        assert_eq!(Envelope::from_bytes(&bytes).unwrap(), env);
    }

    #[test]
    fn bad_magic() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[0] = b'X';
        assert_eq!(Envelope::from_bytes(&bytes), Err(PolyError::BadMagic));
    }

    #[test]
    fn unsupported_version() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[OFFSET_VERSION] = 2;
        fix_crc(&mut bytes);
        assert_eq!(
            Envelope::from_bytes(&bytes),
            Err(PolyError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn unknown_algorithm() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[OFFSET_ALGORITHM] = 7;
        fix_crc(&mut bytes);
        assert_eq!(
            Envelope::from_bytes(&bytes),
            Err(PolyError::UnknownAlgorithm(7))
        );
    }

    #[test]
    fn reserved_flags() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[OFFSET_FLAGS] = 0b10;
        fix_crc(&mut bytes);
        assert_eq!(
            Envelope::from_bytes(&bytes),
            Err(PolyError::ReservedFlags(0b10))
        );
    }

    #[test]
    fn session_flag_is_accepted() {
        let env = Envelope::new(Algorithm::Fake, FLAG_SESSION, CID, vec![1, 2, 3]);
        let parsed = Envelope::from_bytes(&env.to_bytes().unwrap()).unwrap();
        assert!(parsed.is_session());
    }

    #[test]
    fn truncated() {
        let bytes = sample().to_bytes().unwrap();
        for n in [0, 3, 4, 18, 19, bytes.len() - 1] {
            assert_eq!(
                Envelope::from_bytes(&bytes[..n]),
                Err(PolyError::Truncated),
                "len {n}"
            );
        }
    }

    #[test]
    fn huge_payload_len_is_truncated() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[OFFSET_PAYLOAD_LEN..OFFSET_PAYLOAD].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(Envelope::from_bytes(&bytes), Err(PolyError::Truncated));
    }

    #[test]
    fn checksum_mismatch_on_any_flipped_byte() {
        let bytes = sample().to_bytes().unwrap();
        // Every byte except the magic and payload_len (which give BadMagic / Truncated).
        for i in (OFFSET_VERSION..OFFSET_PAYLOAD_LEN).chain(HEADER_LEN..bytes.len()) {
            let mut b = bytes.clone();
            b[i] ^= 0x01;
            assert_eq!(
                Envelope::from_bytes(&b),
                Err(PolyError::ChecksumMismatch),
                "byte {i}"
            );
        }
    }

    #[test]
    fn contact_id_is_sha256_prefix() {
        // SHA-256("abc") = ba7816bf8f01cfea...
        assert_eq!(
            contact_id(b"abc"),
            [0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea]
        );
    }

    proptest! {
        #[test]
        fn from_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = Envelope::from_bytes(&bytes);
        }

        #[test]
        fn from_bytes_never_panics_with_magic(tail in proptest::collection::vec(any::<u8>(), 0..512)) {
            let mut bytes = MAGIC.to_vec();
            bytes.extend(tail);
            let _ = Envelope::from_bytes(&bytes);
        }

        #[test]
        fn roundtrip_any_payload(
            payload in proptest::collection::vec(any::<u8>(), 0..2048),
            cid in any::<[u8; 8]>(),
        ) {
            let env = Envelope::new(Algorithm::MlKem768, 0, cid, payload);
            prop_assert_eq!(Envelope::from_bytes(&env.to_bytes().unwrap()).unwrap(), env);
        }
    }
}
