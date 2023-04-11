use crate::header::Header as Inner;
//use crate::BlueWorkType;
use kaspa_hashes::Hash;
//use kaspa_math::wasm::Uint192;
use std::str::FromStr;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Header {
    inner: Inner,
}

#[wasm_bindgen]
impl Header {
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(constructor)]
    pub fn new(
        version: u16,
        parents_by_level_array: js_sys::Array, //Vec<Vec<Hash>>,
        hash_merkle_root: &str,
        accepted_id_merkle_root: &str,
        utxo_commitment: &str,
        timestamp: u64,
        bits: u32,
        nonce: u64,
        daa_score: u64,
        blue_work: u64,
        blue_score: u64,
        pruning_point: &str,
    ) -> Result<Header, workflow_wasm::error::Error> {
        let mut parents_by_level = vec![];
        for array in parents_by_level_array.iter() {
            parents_by_level.push(Hash::try_vec_from_array(array.into())?);
        }

        Ok(Self {
            inner: Inner::new(
                version,
                parents_by_level,
                Hash::from_str(hash_merkle_root)?,
                Hash::from_str(accepted_id_merkle_root)?,
                Hash::from_str(utxo_commitment)?,
                timestamp,
                bits,
                nonce,
                daa_score,
                blue_work.into(),
                blue_score,
                Hash::from_str(pruning_point)?,
            ),
        })
    }
}

impl Header {
    pub fn inner(&self) -> &Inner {
        &self.inner
    }
}

impl From<Header> for Inner {
    fn from(value: Header) -> Self {
        value.inner
    }
}

impl From<Inner> for Header {
    fn from(inner: Inner) -> Self {
        Self { inner }
    }
}
