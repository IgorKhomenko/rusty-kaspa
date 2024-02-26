//!
//! [`keypair`](mod@self) module encapsulates [`Keypair`] and [`PrivateKey`].
//! The [`Keypair`] provides access to the secret and public keys.
//!
//! ```javascript
//!
//! let keypair = Keypair.random();
//! let privateKey = keypair.privateKey;
//! let publicKey = keypair.publicKey;
//!
//! // to obtain an address from a keypair
//! let address = keypair.toAddress(NetworkType.Mainnnet);
//!
//! // to obtain a keypair from a private key
//! let keypair = privateKey.toKeypair();
//!
//! ```
//!

use crate::imports::*;
use secp256k1::{Secp256k1, XOnlyPublicKey};
use serde_wasm_bindgen::to_value;
use workflow_wasm::abi::*;

/// Data structure that contains a secret and public keys.
/// @category Wallet SDK
#[derive(Debug, Clone)]
#[wasm_bindgen(inspectable)]
pub struct Keypair {
    secret_key: secp256k1::SecretKey,
    public_key: secp256k1::PublicKey,
    xonly_public_key: XOnlyPublicKey,
}

#[wasm_bindgen]
impl Keypair {
    fn new(secret_key: secp256k1::SecretKey, public_key: secp256k1::PublicKey, xonly_public_key: XOnlyPublicKey) -> Self {
        Self { secret_key, public_key, xonly_public_key }
    }

    /// Get the [`PublicKey`] of this [`Keypair`].
    #[wasm_bindgen(getter = publicKey)]
    pub fn get_public_key(&self) -> JsValue {
        to_value(&self.public_key).unwrap()
    }

    /// Get the [`PrivateKey`] of this [`Keypair`].
    #[wasm_bindgen(getter = privateKey)]
    pub fn get_private_key(&self) -> PrivateKey {
        (&self.secret_key).into()
    }

    /// Get the `XOnlyPublicKey` of this [`Keypair`].
    #[wasm_bindgen(getter = xOnlyPublicKey)]
    pub fn get_xonly_public_key(&self) -> JsValue {
        to_value(&self.xonly_public_key).unwrap()
    }

    /// Get the [`Address`] of this Keypair's [`PublicKey`].
    /// Receives a [`NetworkType`] to determine the prefix of the address.
    /// JavaScript: `let address = keypair.toAddress(NetworkType.MAINNET);`.
    #[wasm_bindgen(js_name = toAddress)]
    // pub fn to_address(&self, network_type: NetworkType) -> Result<Address> {
    pub fn to_address(&self, network: INetworkType) -> Result<Address> {
        let pk = PublicKey { xonly_public_key: self.xonly_public_key, source: self.public_key.to_string() };
        let address = pk.to_address(network)?;
        Ok(address)
    }

    /// Get `ECDSA` [`Address`] of this Keypair's [`PublicKey`].
    /// Receives a [`NetworkType`] to determine the prefix of the address.
    /// JavaScript: `let address = keypair.toAddress(NetworkType.MAINNET);`.
    #[wasm_bindgen(js_name = toAddressECDSA)]
    pub fn to_address_ecdsa(&self, network: INetworkType) -> Result<Address> {
        let pk = PublicKey { xonly_public_key: self.xonly_public_key, source: self.public_key.to_string() };
        let address = pk.to_address_ecdsa(network)?;
        Ok(address)
    }

    /// Create a new random [`Keypair`].
    /// JavaScript: `let keypair = Keypair::random();`.
    #[wasm_bindgen]
    pub fn random() -> Result<Keypair, JsError> {
        let secp = Secp256k1::new();
        let (secret_key, public_key) = secp.generate_keypair(&mut rand::thread_rng());
        let (xonly_public_key, _) = public_key.x_only_public_key();
        Ok(Keypair::new(secret_key, public_key, xonly_public_key))
    }

    /// Create a new [`Keypair`] from a [`PrivateKey`].
    /// JavaScript: `let privkey = new PrivateKey(hexString); let keypair = privkey.toKeypair();`.
    #[wasm_bindgen(js_name = "fromPrivateKey")]
    pub fn from_private_key(secret_key: &PrivateKey) -> Result<Keypair, JsError> {
        let secp = Secp256k1::new();
        let secret_key = secp256k1::SecretKey::from_slice(&secret_key.secret_bytes())?;
        let public_key = secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
        let (xonly_public_key, _) = public_key.x_only_public_key();
        Ok(Keypair::new(secret_key, public_key, xonly_public_key))
    }
}

impl TryFrom<JsValue> for Keypair {
    type Error = Error;
    fn try_from(value: JsValue) -> std::result::Result<Self, Self::Error> {
        Ok(ref_from_abi!(Keypair, &value)?)
    }
}