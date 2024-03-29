// Copyright Rivtower Technologies LLC.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::{
    pubkey_to_address, Address, Error, Message, PrivKey, PubKey, SECP256K1, SIGNATURE_BYTES_LEN,
};
use cita_crypto_trait::Sign;
use cita_types::H256;
use rlp::*;
use rustc_serialize::hex::ToHex;
use secp256k1::{PublicKey, SecretKey};
use secp256k1::{
    ecdsa::RecoverableSignature, ecdsa::RecoveryId, Error as SecpError,
    Message as SecpMessage,
};
use serde::de::{Error as SerdeError, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::PartialEq;
use std::convert::From;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

pub struct Signature(pub [u8; 65]);

impl Signature {
    /// Get a slice into the 'r' portion of the data.
    pub fn r(&self) -> &[u8] {
        &self.0[0..32]
    }

    /// Get a slice into the 's' portion of the data.
    pub fn s(&self) -> &[u8] {
        &self.0[32..64]
    }

    /// Get the recovery byte.
    pub fn v(&self) -> u8 {
        self.0[64]
    }

    /// Create a signature object from the sig.
    pub fn from_rsv(r: &H256, s: &H256, v: u8) -> Signature {
        let mut sig = [0u8; 65];
        sig[0..32].copy_from_slice(&r.0);
        sig[32..64].copy_from_slice(&s.0);
        sig[64] = v;
        Signature(sig)
    }

    /// Check if this is a "low" signature.
    pub fn is_low_s(&self) -> bool {
        H256::from_slice(self.s())
            <= H256::from_str("7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0")
                .unwrap()
    }

    /// Check if each component of the signature is in range.
    pub fn is_valid(&self) -> bool {
        self.v() <= 1
            && H256::from_slice(self.r())
                < H256::from_str("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                    .unwrap()
            && H256::from_slice(self.r()) >= H256::from_str("1").unwrap()
            && H256::from_slice(self.s())
                < H256::from_str("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                    .unwrap()
            && H256::from_slice(self.s()) >= H256::from_str("1").unwrap()
    }
}

// manual implementation large arrays don't have trait impls by default.
// remove when integer generics exist
impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.0[..] == other.0[..]
    }
}

impl Decodable for Signature {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        rlp.decoder().decode_value(|bytes| {
            let mut sig = [0u8; 65];
            sig[0..65].copy_from_slice(bytes);
            Ok(Signature(sig))
        })
    }
}

impl Encodable for Signature {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.encoder().encode_value(&self.0[0..65]);
    }
}

// TODO: Maybe it should be implemented with rust macro(https://github.com/rust-lang/rfcs/issues/1038)
impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SignatureVisitor;

        impl<'de> Visitor<'de> for SignatureVisitor {
            type Value = Signature;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("secp256k1 signature")
            }

            fn visit_seq<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let mut signature = Signature([0u8; SIGNATURE_BYTES_LEN]);
                for i in 0..SIGNATURE_BYTES_LEN {
                    signature.0[i] = match visitor.next_element()? {
                        Some(val) => val,
                        None => return Err(SerdeError::invalid_length(SIGNATURE_BYTES_LEN, &self)),
                    }
                }
                Ok(signature)
            }
        }

        let visitor = SignatureVisitor;
        deserializer.deserialize_seq(visitor)
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(SIGNATURE_BYTES_LEN))?;
        for i in 0..SIGNATURE_BYTES_LEN {
            seq.serialize_element(&self.0[i])?;
        }
        seq.end()
    }
}

// manual implementation required in Rust 1.13+, see `std::cmp::AssertParamIsEq`.
impl Eq for Signature {}

// also manual for the same reason, but the pretty printing might be useful.
impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("Signature")
            .field("r", &self.0[0..32].to_hex())
            .field("s", &self.0[32..64].to_hex())
            .field("v", &self.0[64..65].to_hex())
            .finish()
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self.to_hex())
    }
}

impl Default for Signature {
    fn default() -> Self {
        Signature([0; 65])
    }
}

impl Hash for Signature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl Clone for Signature {
    fn clone(&self) -> Self {
        Signature(self.0)
    }
}

impl From<[u8; 65]> for Signature {
    fn from(s: [u8; 65]) -> Self {
        Signature(s)
    }
}

impl From<Signature> for [u8; 65] {
    fn from(s: Signature) -> Self {
        s.0
    }
}

impl<'a> From<&'a [u8]> for Signature {
    fn from(slice: &'a [u8]) -> Signature {
        assert_eq!(slice.len(), SIGNATURE_BYTES_LEN);
        let mut bytes = [0u8; 65];
        bytes.copy_from_slice(slice);
        Signature(bytes)
    }
}

impl<'a> From<&'a Signature> for &'a [u8] {
    fn from(s: &'a Signature) -> Self {
        &s.0[..]
    }
}

impl fmt::LowerHex for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for i in &self.0[..] {
            write!(f, "{:02x}", i)?;
        }
        Ok(())
    }
}

impl From<Signature> for String {
    fn from(s: Signature) -> Self {
        format!("{:x}", s)
    }
}

impl Deref for Signature {
    type Target = [u8; 65];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Signature {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub fn sign(privkey: &PrivKey, message: &Message) -> Result<Signature, Error> {
    let context = &SECP256K1;
    // no way to create from raw byte array.
    let sec: &SecretKey = unsafe { &*(privkey as *const H256 as *const secp256k1::SecretKey) };
    let s = context.sign_ecdsa_recoverable(&SecpMessage::from_slice(&message.0[..])?, sec);
    let (rec_id, data) = s.serialize_compact();
    let mut data_arr = [0; 65];

    // no need to check if s is low, it always is
    data_arr[0..64].copy_from_slice(&data[0..64]);
    data_arr[64] = rec_id.to_i32() as u8;
    Ok(Signature(data_arr))
}

pub fn verify_public(
    pubkey: &PubKey,
    signature: &Signature,
    message: &Message,
) -> Result<bool, Error> {
    let context = &SECP256K1;
    let rsig = RecoverableSignature::from_compact(
        &signature[0..64],
        RecoveryId::from_i32(i32::from(signature[64]))?,
    )?;
    let sig = rsig.to_standard();

    let pdata: [u8; 65] = {
        let mut temp = [4u8; 65];
        temp[1..65].copy_from_slice(pubkey.as_bytes());
        temp
    };

    let public_key = PublicKey::from_slice(&pdata)?;
    match context.verify_ecdsa(&SecpMessage::from_slice(&message.0[..])?, &sig, &public_key) {
        Ok(_) => Ok(true),
        Err(SecpError::IncorrectSignature) => Ok(false),
        Err(x) => Err(Error::from(x)),
    }
}

pub fn verify_address(
    address: &Address,
    signature: &Signature,
    message: &Message,
) -> Result<bool, Error> {
    let pubkey = recover(signature, message)?;
    let recovered_address = pubkey_to_address(&pubkey);
    Ok(address == &recovered_address)
}

pub fn recover(signature: &Signature, message: &Message) -> Result<PubKey, Error> {
    let context = &SECP256K1;
    let rsig = RecoverableSignature::from_compact(
        &signature[0..64],
        RecoveryId::from_i32(i32::from(signature[64]))?,
    )?;
    let publ = context.recover_ecdsa(&SecpMessage::from_slice(&message.0[..])?, &rsig)?;
    let serialized = publ.serialize_uncompressed();

    let mut pubkey = PubKey::default();
    pubkey.0.copy_from_slice(&serialized[1..65]);
    Ok(pubkey)
}

impl Sign for Signature {
    type PrivKey = PrivKey;
    type PubKey = PubKey;
    type Message = Message;
    type Error = Error;
    type Address = Address;

    fn sign(privkey: &Self::PrivKey, message: &Self::Message) -> Result<Self, Self::Error> {
        let context = &SECP256K1;
        // no way to create from raw byte array.
        let sec: &SecretKey = unsafe { &*(privkey as *const H256 as *const secp256k1::SecretKey) };
        let msg = SecpMessage::from_slice(&message.0[..]).unwrap();
        let s = context.sign_ecdsa_recoverable(&msg, sec);
        let (rec_id, data) = s.serialize_compact();
        let mut data_arr = [0; 65];

        // no need to check if s is low, it always is
        data_arr[0..64].copy_from_slice(&data[0..64]);
        data_arr[64] = rec_id.to_i32() as u8;
        Ok(Signature(data_arr))
    }

    fn recover(&self, message: &Message) -> Result<Self::PubKey, Error> {
        let context = &SECP256K1;
        let rsig = RecoverableSignature::from_compact(
            &self.0[0..64],
            RecoveryId::from_i32(i32::from(self.0[64]))?,
        )?;
        let publ = context.recover_ecdsa(&SecpMessage::from_slice(&message.0[..])?, &rsig)?;
        let serialized = publ.serialize_uncompressed();

        let mut pubkey = PubKey::default();
        pubkey.0.copy_from_slice(&serialized[1..65]);
        Ok(pubkey)
    }

    fn verify_public(
        &self,
        pubkey: &Self::PubKey,
        message: &Self::Message,
    ) -> Result<bool, Self::Error> {
        let context = &SECP256K1;
        let rsig = RecoverableSignature::from_compact(
            &self.0[0..64],
            RecoveryId::from_i32(i32::from(self.0[64]))?,
        )?;
        let sig = rsig.to_standard();

        let pdata: [u8; 65] = {
            let mut temp = [4u8; 65];
            temp[1..65].copy_from_slice(pubkey.as_bytes());
            temp
        };

        let publ = PublicKey::from_slice(&pdata)?;
        match context.verify_ecdsa(&SecpMessage::from_slice(&message.0[..])?, &sig, &publ) {
            Ok(_) => Ok(true),
            Err(SecpError::IncorrectSignature) => Ok(false),
            Err(x) => Err(Error::from(x)),
        }
    }

    fn verify_address(
        &self,
        address: &Address,
        message: &Self::Message,
    ) -> Result<bool, Self::Error> {
        let pubkey = self.recover(message)?;
        let recovered_address = pubkey_to_address(&pubkey);
        Ok(address == &recovered_address)
    }
}

#[cfg(test)]
mod tests {
    use super::super::KeyPair;
    use super::{PrivKey, Signature};
    use bincode::{deserialize, serialize};
    use cita_crypto_trait::{CreateKey, Sign};
    use cita_types::H256;
    use hashable::Hashable;
    use std::str::FromStr;

    #[test]
    fn test_sign_verify() {
        let keypair = KeyPair::gen_keypair();
        let str = "".to_owned();
        let message = str.crypt_hash();
        let sig = Signature::sign(keypair.privkey(), &message.into()).unwrap();
        assert!(sig
            .verify_public(keypair.pubkey(), &message.into())
            .unwrap());
    }

    #[test]
    fn test_verify_address() {
        let keypair = KeyPair::gen_keypair();
        let str = "".to_owned();
        let message = str.crypt_hash();
        let sig = Signature::sign(keypair.privkey(), &message.into()).unwrap();
        assert_eq!(keypair.pubkey(), &sig.recover(&message.into()).unwrap());
    }

    #[test]
    fn test_recover() {
        let keypair = KeyPair::gen_keypair();
        let str = "".to_owned();
        let message = str.crypt_hash();
        let sig = Signature::sign(keypair.privkey(), &message.into()).unwrap();
        assert_eq!(keypair.pubkey(), &sig.recover(&message.into()).unwrap());
    }

    #[test]
    fn test_into_slice() {
        let keypair = KeyPair::gen_keypair();
        let str = "".to_owned();
        let message = str.crypt_hash();
        let sig = Signature::sign(keypair.privkey(), &message.into()).unwrap();
        let sig = &sig;
        let slice: &[u8] = sig.into();
        assert_eq!(Signature::from(slice), *sig);
    }

    #[test]
    fn test_de_serialize() {
        let keypair = KeyPair::gen_keypair();
        let str = "".to_owned();
        let message = str.crypt_hash();
        let signature = Signature::sign(keypair.privkey().into(), &message.into()).unwrap();
        let se_result = serialize(&signature).unwrap();
        let de_result: Signature = deserialize(&se_result).unwrap();
        assert_eq!(signature, de_result);
    }

    #[test]
    fn test_show_signature() {
        let sk = PrivKey::from(
            H256::from_str("80762b900f072d199e35ea9b1ee0e2e631a87762f8855b32d4ec13e37a3a65c1")
                .unwrap(),
        );
        let str = "".to_owned();
        let message = str.crypt_hash();
        println!("message {:?}", message);
        let signature = Signature::sign(&sk, &message.into()).unwrap();
        println!("signature {:?}", signature);
    }
}
